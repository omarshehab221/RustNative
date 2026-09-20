//! Running Cargo.
//!
//! `rf` orchestrates the native toolchains rather than replacing them, and
//! Cargo is the first of them: `rf build windows` *is* `cargo build`, with
//! the project's own manifest and whatever Cargo already knows about the
//! host's linker. Output is inherited, so what a person sees is Cargo's own
//! progress and diagnostics, not a paraphrase.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};

/// Runs `cargo <arguments>` in `directory`, with its output inherited.
///
/// # Errors
///
/// [`Error::ToolMissing`] if Cargo is not on the path, or
/// [`Error::ToolFailed`] if it exits non-zero.
pub fn run<I, S>(directory: &Path, arguments: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(cargo());
    command.current_dir(directory).args(arguments);
    let status = command.status().map_err(|cause| Error::ToolMissing {
        tool: "cargo",
        hint: "install Rust from https://rustup.rs".to_owned(),
        cause: Some(cause.to_string()),
    })?;
    if status.success() {
        return Ok(());
    }
    Err(Error::ToolFailed { tool: "cargo", code: status.code() })
}

/// The Cargo to run: the one that invoked `rf` if there was one (so
/// `cargo run -p rf-cli -- build` uses the same toolchain), otherwise
/// whatever is on the path.
fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}
