//! `compile_styles()`: the project's style file, compiled into a theme
//! (`PLAN.md` Milestone 58, "The style file").
//!
//! Reads `app.css` (or the file `rustnative.toml`'s `[style] file` names),
//! validates it — a mistake fails the build at its file, line, and column —
//! and writes `OUT_DIR/app_theme.rs`, which `framework_core::app_theme!()`
//! includes: the default theme with the file's tokens written over it, the
//! tokens still references so a later switch is a re-resolution.

use std::path::{Path, PathBuf};

use framework_style::Vocabulary;
use quote::quote;

/// The style file for the crate at `project`, if there is one.
#[must_use]
pub fn style_file(project: &Path) -> Option<PathBuf> {
    #[derive(serde::Deserialize, Default)]
    struct Config {
        #[serde(default)]
        style: Option<Style>,
    }
    #[derive(serde::Deserialize)]
    struct Style {
        file: Option<PathBuf>,
    }
    let configured = std::fs::read_to_string(project.join("rustnative.toml"))
        .ok()
        .and_then(|text| toml::from_str::<Config>(&text).ok())
        .and_then(|config| config.style)
        .and_then(|style| style.file);
    let path = project.join(configured.unwrap_or_else(|| PathBuf::from("app.css")));
    path.is_file().then_some(path)
}

/// The Rust source of `app_theme.rs` for a style file's contents.
///
/// # Errors
///
/// Every problem in the file, as `path:line:column: message` lines.
pub fn theme_source(path: &Path, source: &str) -> Result<String, String> {
    let vocabulary = Vocabulary::with_style_file(source).map_err(|errors| {
        errors
            .iter()
            .map(|error| {
                let (line, column) = error.line_column(source);
                format!("{}:{line}:{column}: {}", path.display(), error.message)
            })
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let (cleared, set) = vocabulary.project_theme();
    let tokens = framework_style::tokens::token_table(&cleared, &set, &quote!(::framework_core));
    Ok(quote!(::framework_core::Theme::default().with_tokens(#tokens)).to_string())
}

/// Compiles the project's style file into `OUT_DIR/app_theme.rs`. With no
/// style file, the theme is the default one.
///
/// # Panics
///
/// When the style file does not compile — a mistake in the project, which
/// must stop the build — with every error by file, line, and column.
pub fn compile_styles() {
    let project = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).unwrap_or_default();
    let out = std::env::var_os("OUT_DIR").map(PathBuf::from).unwrap_or_default();
    println!("cargo:rerun-if-changed=rustnative.toml");
    let source = match style_file(&project) {
        Some(path) => {
            println!("cargo:rerun-if-changed={}", path.display());
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|cause| panic!("reading {}: {cause}", path.display()));
            theme_source(&path, &text)
                .unwrap_or_else(|errors| panic!("the style file does not compile:\n{errors}"))
        }
        None => "::framework_core::Theme::default()".to_owned(),
    };
    let target = out.join("app_theme.rs");
    if std::fs::read_to_string(&target).ok().as_deref() != Some(source.as_str()) {
        std::fs::write(&target, source)
            .unwrap_or_else(|cause| panic!("writing {}: {cause}", target.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_file_becomes_a_theme_expression() {
        let source = "@theme {\n  --color-*: initial;\n  --color-primary: #0a84ff;\n}\n";
        let theme = theme_source(Path::new("app.css"), source).unwrap();
        assert!(theme.contains("without_namespace (\"color-\")"), "{theme}");
        assert!(theme.contains("\"color-primary\""), "{theme}");
    }

    #[test]
    fn a_mistake_is_reported_at_its_position() {
        let errors = theme_source(Path::new("app.css"), "\n.button { color: red; }\n").unwrap_err();
        assert!(errors.starts_with("app.css:2:1:"), "{errors}");
    }
}
