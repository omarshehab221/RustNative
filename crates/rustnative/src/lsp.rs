//! `rustnative lsp`: editor support for `.rsx` files that is a proxy, not a
//! fork (`PLAN.md` Milestone 53).
//!
//! The editor talks to `rustnative lsp`; `rustnative lsp` talks to
//! `rust-analyzer`. A `.rsx` document the editor opens is compiled in
//! memory and presented to the Rust language server as its lowered file —
//! the same file the build script writes into `OUT_DIR` — and every
//! position crossing the proxy is mapped through the source map, in both
//! directions: requests (completion, hover, go-to-definition, rename) move
//! from `.rsx` positions to lowered ones, and results and diagnostics move
//! back. The proxy answers only what is markup: element and attribute
//! completion inside a tag, and hover that names the builder method an
//! attribute calls.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use framework_markup::{CompileOptions, SourceMap, compile, element_spec, element_table};
use serde_json::{Value, json};

use crate::error::{Error, Result};

/// One open `.rsx` document.
#[derive(Debug, Clone)]
struct Document {
    source: String,
    lowered_uri: String,
    map: SourceMap,
}

/// What the proxy knows about open documents.
#[derive(Debug, Default)]
pub struct State {
    documents: HashMap<String, Document>,
    lowered_to_source: HashMap<String, String>,
}

/// Reads one LSP message (`Content-Length` framing).
///
/// # Errors
///
/// The stream ended or the framing is malformed.
pub fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = length else {
        return Err(std::io::Error::other("an LSP message without Content-Length"));
    };
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(std::io::Error::other)
}

/// Writes one LSP message.
///
/// # Errors
///
/// The stream could not be written.
pub fn write_message(writer: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let body = message.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    writer.flush()
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file:///").or_else(|| uri.strip_prefix("file://"))?;
    let decoded = percent_decode(path);
    // `file:///C:/x` on Windows; `file:///home/x` elsewhere.
    if decoded.len() > 1 && decoded.as_bytes().get(1) == Some(&b':') {
        Some(PathBuf::from(decoded))
    } else {
        Some(PathBuf::from(format!("/{decoded}")))
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if let Some(byte) =
                text.get(index + 1..index + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn path_to_uri(path: &Path) -> String {
    let text = path.display().to_string().replace('\\', "/");
    if text.starts_with('/') { format!("file://{text}") } else { format!("file:///{text}") }
}

/// Where the build script writes `source`'s lowered file: the newest
/// `target/*/build/*/out/rsx/<relative>.rs` for the crate containing it.
fn lowered_path(source: &Path) -> PathBuf {
    let crate_root =
        source.ancestors().find(|ancestor| ancestor.join("Cargo.toml").is_file()).map_or_else(
            || source.parent().map_or_else(PathBuf::new, Path::to_path_buf),
            Path::to_path_buf,
        );
    let relative =
        source.strip_prefix(crate_root.join("src")).unwrap_or(source).with_extension("rs");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for target in crate_root
        .ancestors()
        .map(|ancestor| ancestor.join("target"))
        .filter(|target| target.is_dir())
    {
        for profile in std::fs::read_dir(&target).into_iter().flatten().flatten() {
            let build = profile.path().join("build");
            for package in std::fs::read_dir(&build).into_iter().flatten().flatten() {
                let candidate = package.path().join("out").join("rsx").join(&relative);
                if let Ok(modified) = candidate.metadata().and_then(|meta| meta.modified()) {
                    if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
                        newest = Some((modified, candidate));
                    }
                }
            }
        }
    }
    // Before the first build, a stable place beside the target directory.
    newest
        .map_or_else(|| crate_root.join("target").join("rsx-lsp").join(&relative), |(_, path)| path)
}

fn position(value: &Value) -> Option<(usize, usize)> {
    let line = usize::try_from(value.get("line")?.as_u64()?).ok()?;
    let character = usize::try_from(value.get("character")?.as_u64()?).ok()?;
    Some((line, character))
}

fn set_position(value: &mut Value, line: usize, character: usize) {
    value["line"] = json!(line);
    value["character"] = json!(character);
}

/// LSP positions are 0-based; the source map's are 1-based.
fn to_lowered(map: &SourceMap, value: &mut Value) {
    if let Some((line, character)) = position(value) {
        let (_, lowered) = map.to_lowered(line + 1, character + 1);
        set_position(value, line, lowered.saturating_sub(1));
    }
}

fn to_source(map: &SourceMap, value: &mut Value) {
    if let Some((line, character)) = position(value) {
        let (_, source) = map.to_source(line + 1, character + 1);
        set_position(value, line, source.saturating_sub(1));
    }
}

fn map_range(map: &SourceMap, range: &mut Value, towards_source: bool) {
    for end in ["start", "end"] {
        if let Some(position) = range.get_mut(end) {
            if towards_source { to_source(map, position) } else { to_lowered(map, position) }
        }
    }
}

impl State {
    fn open(&mut self, uri: &str, text: &str) -> Option<Document> {
        let path = uri_to_path(uri)?;
        let src_root = path
            .ancestors()
            .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "src"))
            .map_or_else(
                || path.parent().map_or_else(PathBuf::new, Path::to_path_buf),
                Path::to_path_buf,
            );
        let options = CompileOptions { source_path: path.clone(), src_root, wrapper: None };
        let output = compile(text, &options).ok()?;
        let lowered_uri = path_to_uri(&lowered_path(&path));
        let document =
            Document { source: text.to_owned(), lowered_uri: lowered_uri.clone(), map: output.map };
        self.lowered_to_source.insert(lowered_uri, uri.to_owned());
        self.documents.insert(uri.to_owned(), document.clone());
        Some(document).map(|mut document| {
            document.source = output.code;
            document
        })
    }

    /// Rewrites a client message for the server. Returns the message to
    /// forward, or — for what the proxy answers itself — a response for the
    /// client instead.
    pub fn route_client(&mut self, mut message: Value) -> Routed {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("").to_owned();
        let uri =
            message.pointer("/params/textDocument/uri").and_then(Value::as_str).map(str::to_owned);
        let Some(uri) = uri.filter(|uri| {
            std::path::Path::new(uri)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rsx"))
        }) else {
            return Routed::Server(message);
        };
        match method.as_str() {
            "textDocument/didOpen" => {
                let text = message
                    .pointer("/params/textDocument/text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let version = message
                    .pointer("/params/textDocument/version")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let Some(lowered) = self.open(&uri, &text) else { return Routed::Drop };
                message["params"]["textDocument"] = json!({
                    "uri": self.documents[&uri].lowered_uri,
                    "languageId": "rust",
                    "version": version,
                    "text": lowered.source,
                });
                Routed::Server(message)
            }
            "textDocument/didChange" => {
                let Some(mut text) =
                    self.documents.get(&uri).map(|document| document.source.clone())
                else {
                    return Routed::Drop;
                };
                for change in message
                    .pointer("/params/contentChanges")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                {
                    let new_text = change.get("text").and_then(Value::as_str).unwrap_or("");
                    match change.get("range") {
                        None => new_text.clone_into(&mut text),
                        Some(range) => apply_change(&mut text, range, new_text),
                    }
                }
                let version = message
                    .pointer("/params/textDocument/version")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let Some(lowered) = self.open(&uri, &text) else { return Routed::Drop };
                message["params"]["textDocument"] =
                    json!({ "uri": self.documents[&uri].lowered_uri, "version": version });
                message["params"]["contentChanges"] = json!([{ "text": lowered.source }]);
                Routed::Server(message)
            }
            "textDocument/completion" => {
                if let Some(items) = self.markup_completion(&uri, &message) {
                    return Routed::Client(
                        json!({ "jsonrpc": "2.0", "id": message["id"], "result": items }),
                    );
                }
                self.forward_with_positions(&uri, message)
            }
            "textDocument/hover" => {
                if let Some(hover) = self.markup_hover(&uri, &message) {
                    return Routed::Client(
                        json!({ "jsonrpc": "2.0", "id": message["id"], "result": hover }),
                    );
                }
                self.forward_with_positions(&uri, message)
            }
            _ => self.forward_with_positions(&uri, message),
        }
    }

    fn forward_with_positions(&self, uri: &str, mut message: Value) -> Routed {
        let Some(document) = self.documents.get(uri) else { return Routed::Server(message) };
        message["params"]["textDocument"]["uri"] = json!(document.lowered_uri);
        if let Some(position) = message.pointer_mut("/params/position") {
            to_lowered(&document.map, position);
        }
        if let Some(range) = message.pointer_mut("/params/range") {
            map_range(&document.map, range, false);
        }
        Routed::Server(message)
    }

    /// Rewrites a server message for the client: every lowered URI becomes
    /// its `.rsx` URI, with the positions beside it mapped back.
    pub fn route_server(&self, mut message: Value) -> Value {
        if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
        {
            let lowered =
                message.pointer("/params/uri").and_then(Value::as_str).unwrap_or("").to_owned();
            if let Some(source_uri) = self.lowered_to_source.get(&lowered) {
                let map = &self.documents[source_uri].map;
                message["params"]["uri"] = json!(source_uri);
                if let Some(diagnostics) =
                    message.pointer_mut("/params/diagnostics").and_then(Value::as_array_mut)
                {
                    for diagnostic in diagnostics {
                        if let Some(range) = diagnostic.get_mut("range") {
                            map_range(map, range, true);
                        }
                    }
                }
            }
            return message;
        }
        self.rewrite_locations(&mut message);
        message
    }

    fn rewrite_locations(&self, value: &mut Value) {
        match value {
            Value::Object(object) => {
                for (uri_key, range_keys) in [
                    ("uri", &["range"][..]),
                    ("targetUri", &["targetRange", "targetSelectionRange"][..]),
                ] {
                    let lowered = object.get(uri_key).and_then(Value::as_str).map(str::to_owned);
                    if let Some(source_uri) =
                        lowered.and_then(|lowered| self.lowered_to_source.get(&lowered))
                    {
                        let map = &self.documents[source_uri].map;
                        object.insert(uri_key.to_owned(), json!(source_uri));
                        for key in range_keys {
                            if let Some(range) = object.get_mut(*key) {
                                map_range(map, range, true);
                            }
                        }
                    }
                }
                for child in object.values_mut() {
                    self.rewrite_locations(child);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.rewrite_locations(item)),
            _ => {}
        }
    }

    /// The text of line `line` up to `character`, in the `.rsx` document.
    fn prefix(&self, uri: &str, message: &Value) -> Option<String> {
        let document = self.documents.get(uri)?;
        let (line, character) = position(message.pointer("/params/position")?)?;
        let text = document.source.lines().nth(line)?;
        Some(text.chars().take(character).collect())
    }

    fn markup_completion(&self, uri: &str, message: &Value) -> Option<Value> {
        let prefix = self.prefix(uri, message)?;
        match tag_context(&prefix)? {
            TagContext::ElementName => Some(json!(element_table()
                .iter()
                .map(|spec| json!({ "label": spec.name, "kind": 7, "detail": format!("Node::{:?}", spec.constructor) }))
                .collect::<Vec<_>>())),
            TagContext::Attribute(element) => {
                let spec = element_spec(&element)?;
                Some(json!(spec
                    .attrs
                    .iter()
                    .map(|attr| json!({ "label": attr.name, "kind": 10, "detail": attr.method }))
                    .collect::<Vec<_>>()))
            }
        }
    }

    fn markup_hover(&self, uri: &str, message: &Value) -> Option<Value> {
        let document = self.documents.get(uri)?;
        let (line, character) = position(message.pointer("/params/position")?)?;
        let text: Vec<char> = document.source.lines().nth(line)?.chars().collect();
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let start =
            (0..character.min(text.len())).rev().take_while(|&i| is_word(text[i])).last()?;
        let end = (character..text.len())
            .take_while(|&i| is_word(text[i]))
            .last()
            .map_or(character, |i| i + 1);
        let word: String = text[start..end].iter().collect();
        let before: String = text[..start].iter().collect();
        let TagContext::Attribute(element) = tag_context(&before)? else { return None };
        let attr = element_spec(&element)?.attrs.into_iter().find(|attr| attr.name == word)?;
        Some(
            json!({ "contents": { "kind": "markdown", "value": format!("`{word}` → `{}`", attr.method) } }),
        )
    }
}

/// Where the cursor is inside markup.
#[derive(Debug, PartialEq, Eq)]
enum TagContext {
    /// Just after `<`: an element name.
    ElementName,
    /// Inside `<Element …`: an attribute of that element.
    Attribute(String),
}

fn tag_context(prefix: &str) -> Option<TagContext> {
    let open = prefix.rfind('<')?;
    let after = &prefix[open + 1..];
    if after.contains('>') || after.starts_with('/') {
        return None;
    }
    let name: String =
        after.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':').collect();
    if name.len() == after.len() {
        return Some(TagContext::ElementName);
    }
    // Inside a braced value the cursor is in Rust, not markup.
    if after.matches('{').count() > after.matches('}').count() {
        return None;
    }
    Some(TagContext::Attribute(name))
}

fn apply_change(text: &mut String, range: &Value, new_text: &str) {
    let offset = |position: &Value| -> Option<usize> {
        let (line, character) = self::position(position)?;
        let mut offset = 0;
        for (index, current) in text.split_inclusive('\n').enumerate() {
            if index == line {
                return Some(
                    offset + current.chars().take(character).map(char::len_utf8).sum::<usize>(),
                );
            }
            offset += current.len();
        }
        Some(text.len())
    };
    if let (Some(start), Some(end)) =
        (range.get("start").and_then(offset), range.get("end").and_then(offset))
    {
        if start <= end && end <= text.len() {
            text.replace_range(start..end, new_text);
        }
    }
}

/// Where a client message goes.
#[derive(Debug)]
pub enum Routed {
    /// Forward to the server.
    Server(Value),
    /// Answer the client directly.
    Client(Value),
    /// Nothing to send (a document that does not compile yet).
    Drop,
}

/// Runs the proxy on stdin/stdout, forwarding to `server`.
///
/// # Errors
///
/// The server could not be started.
pub fn serve(server: &str) -> Result<()> {
    let mut parts = server.split_whitespace();
    let program = parts.next().unwrap_or("rust-analyzer");
    let mut child = Command::new(program)
        .args(parts)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|cause| Error::ToolMissing {
            tool: "rust-analyzer",
            hint: "rustup component add rust-analyzer".to_owned(),
            cause: Some(cause.to_string()),
        })?;
    let state = Arc::new(Mutex::new(State::default()));
    let server_in = Arc::new(Mutex::new(
        child.stdin.take().ok_or_else(|| Error::Usage("no server stdin".into()))?,
    ));
    let server_out = child.stdout.take().ok_or_else(|| Error::Usage("no server stdout".into()))?;
    let client_out = Arc::new(Mutex::new(std::io::stdout()));

    let to_client = {
        let state = Arc::clone(&state);
        let client_out = Arc::clone(&client_out);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(server_out);
            while let Ok(Some(message)) = read_message(&mut reader) {
                let message =
                    state.lock().unwrap_or_else(PoisonError::into_inner).route_server(message);
                let mut out = client_out.lock().unwrap_or_else(PoisonError::into_inner);
                if write_message(&mut *out, &message).is_err() {
                    break;
                }
            }
        })
    };

    let mut reader = BufReader::new(std::io::stdin());
    while let Ok(Some(message)) = read_message(&mut reader) {
        let exiting = message.get("method").and_then(Value::as_str) == Some("exit");
        let routed = state.lock().unwrap_or_else(PoisonError::into_inner).route_client(message);
        match routed {
            Routed::Server(message) => {
                let mut input = server_in.lock().unwrap_or_else(PoisonError::into_inner);
                if write_message(&mut *input, &message).is_err() {
                    break;
                }
            }
            Routed::Client(message) => {
                let mut out = client_out.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = write_message(&mut *out, &message);
            }
            Routed::Drop => {}
        }
        if exiting {
            break;
        }
    }
    drop(server_in);
    let _ = child.wait();
    let _ = to_client.join();
    Ok(())
}

/// A stand-in language server for tests: answers every request with its
/// own params, and publishes one diagnostic at the position of the first
/// `gap` in every document it is given.
///
/// # Errors
///
/// Never in practice; stdin/stdout failures end it.
#[allow(clippy::unnecessary_wraps, reason = "the same shape as every other command handler")]
pub fn echo_server() -> Result<()> {
    let mut reader = BufReader::new(std::io::stdin());
    let mut out = std::io::stdout();
    while let Ok(Some(message)) = read_message(&mut reader) {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "exit" {
            break;
        }
        if method == "textDocument/didOpen" {
            let uri = message.pointer("/params/textDocument/uri").cloned().unwrap_or(Value::Null);
            let text =
                message.pointer("/params/textDocument/text").and_then(Value::as_str).unwrap_or("");
            let (line, character) = text
                .lines()
                .enumerate()
                .find_map(|(line, content)| {
                    content.find("gap").map(|at| (line, content[..at].chars().count()))
                })
                .unwrap_or((0, 0));
            let notification = json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": { "uri": uri, "diagnostics": [{
                    "range": { "start": { "line": line, "character": character }, "end": { "line": line, "character": character + 3 } },
                    "message": "echo diagnostic"
                }]}
            });
            let _ = write_message(&mut out, &notification);
        } else if let Some(id) = message.get("id") {
            let response =
                json!({ "jsonrpc": "2.0", "id": id, "result": { "echo": message["params"] } });
            let _ = write_message(&mut out, &response);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_context_distinguishes_names_attributes_and_rust() {
        assert_eq!(tag_context("    <Col"), Some(TagContext::ElementName));
        assert_eq!(
            tag_context("    <Column key=\"a\" pad"),
            Some(TagContext::Attribute("Column".into()))
        );
        assert_eq!(tag_context("    <Column key={value."), None, "inside braces is Rust");
        assert_eq!(tag_context("    let x = a"), None);
        assert_eq!(tag_context("    </Col"), None);
    }

    #[test]
    fn messages_round_trip_through_framing() {
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} });
        let mut buffer = Vec::new();
        write_message(&mut buffer, &message).expect("write");
        let read =
            read_message(&mut BufReader::new(buffer.as_slice())).expect("read").expect("a message");
        assert_eq!(read, message);
    }

    #[test]
    fn open_documents_are_lowered_and_positions_map_both_ways() {
        let root = std::env::temp_dir().join(format!("rustnative-lsp-{}", std::process::id()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("dirs");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("write");
        let path = src.join("app.rsx");
        let uri = path_to_uri(&path);
        let text = "fn f() -> Node {\n    <Column key=\"a\" gap=4></Column>\n}\n";
        let mut state = State::default();
        let open = json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "rsx", "version": 1, "text": text } } });
        let Routed::Server(forwarded) = state.route_client(open) else { panic!("forwarded") };
        let lowered_uri = forwarded
            .pointer("/params/textDocument/uri")
            .and_then(Value::as_str)
            .expect("uri")
            .to_owned();
        assert!(
            lowered_uri.ends_with("/rsx/app.rs") || lowered_uri.ends_with("/rsx-lsp/app.rs"),
            "{lowered_uri}"
        );
        let lowered_text =
            forwarded.pointer("/params/textDocument/text").and_then(Value::as_str).expect("text");
        assert!(lowered_text.contains("::framework_core::rsx!(<Column"));

        // A hover on `gap` moves right by the inserted prefix...
        let gap = text.lines().nth(1).expect("line").find("gap").expect("gap");
        let hover = json!({ "jsonrpc": "2.0", "id": 2, "method": "textDocument/definition", "params": { "textDocument": { "uri": uri }, "position": { "line": 1, "character": gap } } });
        let Routed::Server(forwarded) = state.route_client(hover) else { panic!("forwarded") };
        let lowered_gap = lowered_text.lines().nth(1).expect("line").find("gap").expect("gap");
        assert_eq!(forwarded.pointer("/params/position/character"), Some(&json!(lowered_gap)));

        // ...and a diagnostic on the lowered `gap` comes back to the `.rsx` one.
        let published = json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": lowered_uri, "diagnostics": [{ "range": { "start": { "line": 1, "character": lowered_gap }, "end": { "line": 1, "character": lowered_gap + 3 } }, "message": "x" }] } });
        let back = state.route_server(published);
        assert_eq!(back.pointer("/params/uri"), Some(&json!(uri)));
        assert_eq!(back.pointer("/params/diagnostics/0/range/start/character"), Some(&json!(gap)));

        // Completion inside a tag is answered by the proxy itself.
        let complete = json!({ "jsonrpc": "2.0", "id": 3, "method": "textDocument/completion", "params": { "textDocument": { "uri": uri }, "position": { "line": 1, "character": gap } } });
        let Routed::Client(answer) = state.route_client(complete) else { panic!("answered") };
        let labels: Vec<&str> = answer["result"]
            .as_array()
            .expect("items")
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect();
        assert!(labels.contains(&"padding") && labels.contains(&"gap"), "{labels:?}");

        // Hover on an attribute names its builder method.
        let hover = json!({ "jsonrpc": "2.0", "id": 4, "method": "textDocument/hover", "params": { "textDocument": { "uri": uri }, "position": { "line": 1, "character": gap + 1 } } });
        let Routed::Client(answer) = state.route_client(hover) else { panic!("answered") };
        assert!(
            answer
                .pointer("/result/contents/value")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("ColumnStyle::gap"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
