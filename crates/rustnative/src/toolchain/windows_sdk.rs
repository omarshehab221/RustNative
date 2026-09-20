//! Finding the Microsoft toolchain: the MSVC build tools Cargo links with,
//! and the Windows SDK tools packaging needs.
//!
//! Nothing here *runs* a build — Cargo does that. This is what `rustnative doctor`
//! reports and what Milestone 32's packaging will call: where `makeappx`
//! and `signtool` are, and whether a linker exists at all.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One tool that was looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// Its file name, without the extension (`makeappx`).
    pub name: &'static str,
    /// Where it was found.
    pub path: Option<PathBuf>,
}

impl Tool {
    /// Whether it was found.
    #[must_use]
    pub const fn found(&self) -> bool {
        self.path.is_some()
    }
}

/// What the Microsoft toolchain looks like on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowsToolchain {
    /// The Visual Studio installation the C++ build tools are in.
    pub visual_studio: Option<PathBuf>,
    /// The Windows SDK version whose tools were found (`10.0.26100.0`).
    pub sdk_version: Option<String>,
    /// The SDK tools packaging and resource compilation need.
    pub tools: Vec<Tool>,
}

impl WindowsToolchain {
    /// Looks for the toolchain.
    #[must_use]
    pub fn detect() -> Self {
        let sdk = latest_sdk_bin();
        let tools = ["rc", "mt", "makeappx", "signtool"]
            .into_iter()
            .map(|name| Tool { name, path: sdk.as_ref().and_then(|bin| in_directory(bin, name)) })
            .collect();
        Self {
            visual_studio: visual_studio(),
            sdk_version: sdk
                .as_ref()
                .and_then(|bin| bin.parent()?.file_name())
                .map(|version| version.to_string_lossy().into_owned()),
            tools,
        }
    }

    /// A tool by name, if it was looked for.
    #[must_use]
    pub fn tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    /// Whether everything a Windows build needs is present: a linker, and
    /// the SDK's resource and manifest tools.
    #[must_use]
    pub fn can_build(&self) -> bool {
        self.visual_studio.is_some()
            && ["rc", "mt"].iter().all(|name| self.tool(name).is_some_and(Tool::found))
    }

    /// Whether everything packaging needs is present as well.
    #[must_use]
    pub fn can_package(&self) -> bool {
        self.can_build()
            && ["makeappx", "signtool"].iter().all(|name| self.tool(name).is_some_and(Tool::found))
    }
}

/// The Visual Studio installation with the C++ tools, according to
/// `vswhere` — the supported way to ask, and the one Cargo's own linker
/// discovery uses.
fn visual_studio() -> Option<PathBuf> {
    let vswhere = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?)
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if !vswhere.is_file() {
        return None;
    }
    let output = Command::new(vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
            "-nologo",
        ])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// The `bin\<version>\<arch>` folder of the newest installed Windows SDK.
fn latest_sdk_bin() -> Option<PathBuf> {
    let roots = ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("Windows Kits").join("10").join("bin"));
    let architecture = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let mut best: Option<(String, PathBuf)> = None;
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let version = entry.file_name().to_string_lossy().into_owned();
            // SDK versions are `10.0.<build>.0` folders; anything else in
            // there (`x64`, `arm64`) is an older, unversioned layout.
            if !version.starts_with("10.") {
                continue;
            }
            let bin = entry.path().join(architecture);
            if !bin.is_dir() {
                continue;
            }
            if best.as_ref().is_none_or(|(newest, _)| version_is_newer(&version, newest)) {
                best = Some((version, bin));
            }
        }
    }
    best.map(|(_, bin)| bin)
}

/// Compares two dotted version folder names numerically, so `10.0.26100.0`
/// is newer than `10.0.9999.0` — which a string comparison gets wrong.
fn version_is_newer(candidate: &str, current: &str) -> bool {
    let parts = |version: &str| {
        version.split('.').map(|part| part.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>()
    };
    parts(candidate) > parts(current)
}

fn in_directory(directory: &Path, name: &str) -> Option<PathBuf> {
    let path = directory.join(format!("{name}.exe"));
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared_by_number_not_by_text() {
        assert!(version_is_newer("10.0.26100.0", "10.0.9999.0"));
        assert!(!version_is_newer("10.0.19041.0", "10.0.26100.0"));
    }

    #[test]
    fn detection_reports_every_tool_it_looked_for() {
        let toolchain = WindowsToolchain::detect();
        let names = toolchain.tools.iter().map(|tool| tool.name).collect::<Vec<_>>();
        assert_eq!(names, vec!["rc", "mt", "makeappx", "signtool"]);
        // Whether they exist depends on the machine; that they were looked
        // for in a place that exists does not.
        for tool in &toolchain.tools {
            if let Some(path) = &tool.path {
                assert!(path.is_file(), "{} was reported at {}", tool.name, path.display());
            }
        }
    }
}
