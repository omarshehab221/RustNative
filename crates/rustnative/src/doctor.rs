//! `rustnative doctor`: what this machine can build, and what it is missing.
//!
//! Every check reports what it actually found — a version, a path — rather
//! than a tick, so a failing build can be compared against what `doctor`
//! said. `--json` prints the same findings for a script to read.

use serde::Serialize;

use crate::platform::Platform;
use crate::toolchain::{self, RustToolchain, windows_sdk::WindowsToolchain};

/// One thing that was checked.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// What was checked ("rustc", "makeappx").
    pub name: String,
    /// Whether it is usable.
    pub ok: bool,
    /// What was found: a version, a path, or why not.
    pub detail: String,
}

/// Whether one platform can be built here.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformReadiness {
    /// The platform.
    pub platform: String,
    /// Whether `rustnative build <platform>` would work on this machine.
    pub ready: bool,
    /// Why, or why not.
    pub detail: String,
}

/// Everything `doctor` found.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The toolchain checks.
    pub checks: Vec<Check>,
    /// Per-platform readiness.
    pub platforms: Vec<PlatformReadiness>,
}

impl Report {
    /// Runs every check.
    #[must_use]
    pub fn gather() -> Self {
        let rust = RustToolchain::detect();
        let windows = WindowsToolchain::detect();
        let mut checks = vec![
            Check {
                name: "rustc".to_owned(),
                ok: rust.meets_minimum,
                detail: match (&rust.version, rust.meets_minimum) {
                    (Some(version), true) => version.clone(),
                    (Some(version), false) => {
                        let (major, minor) = toolchain::MINIMUM_RUST;
                        format!("{version} is older than the required {major}.{minor}")
                    }
                    (None, _) => "not found — install Rust from https://rustup.rs".to_owned(),
                },
            },
            Check {
                name: "cargo".to_owned(),
                ok: toolchain::is_available("cargo"),
                detail: "the build `rustnative` drives".to_owned(),
            },
            Check {
                name: "git".to_owned(),
                ok: toolchain::is_available("git"),
                detail: "optional: only needed to publish or fetch git dependencies".to_owned(),
            },
            Check {
                name: "visual studio (c++ build tools)".to_owned(),
                ok: windows.visual_studio.is_some(),
                detail: windows.visual_studio.as_ref().map_or_else(
                    || {
                        "not found — install the \"Desktop development with C++\" workload"
                            .to_owned()
                    },
                    |path| path.display().to_string(),
                ),
            },
            Check {
                name: "windows sdk".to_owned(),
                ok: windows.sdk_version.is_some(),
                detail: windows
                    .sdk_version
                    .clone()
                    .unwrap_or_else(|| "not found — install the Windows 10/11 SDK".to_owned()),
            },
        ];
        checks.extend(windows.tools.iter().map(|tool| Check {
            name: tool.name.to_owned(),
            ok: tool.found(),
            detail: tool.path.as_ref().map_or_else(
                || "not found in the Windows SDK".to_owned(),
                |path| path.display().to_string(),
            ),
        }));

        let platforms = [
            Platform::Windows,
            Platform::Macos,
            Platform::Linux,
            Platform::Android,
            Platform::Ios,
            Platform::Embedded,
            Platform::Web,
        ]
        .into_iter()
        .map(|platform| {
            let (ready, detail) = match platform.planned_milestone() {
                _ if platform == Platform::Windows => {
                    if windows.can_build() {
                        let packaging = if windows.can_package() {
                            "packaging tools present"
                        } else {
                            "packaging tools missing (makeappx/signtool)"
                        };
                        (true, format!("ready — {packaging}"))
                    } else {
                        (false, "missing the MSVC build tools or the Windows SDK".to_owned())
                    }
                }
                Some(milestone) => (false, format!("no backend yet — Milestone {milestone}")),
                None => (false, "no backend yet — see PLAN.md's web roadmap".to_owned()),
            };
            PlatformReadiness { platform: platform.to_string(), ready, detail }
        })
        .collect();

        Self { checks, platforms }
    }

    /// Whether everything required is present (optional checks aside).
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.checks.iter().filter(|check| check.name != "git").all(|check| check.ok)
    }

    /// The report as a person reads it.
    #[must_use]
    pub fn to_text(&self) -> String {
        use std::fmt::Write as _;

        let mut out = String::from("Toolchains\n");
        for check in &self.checks {
            let mark = if check.ok { "ok  " } else { "MISS" };
            let _ = writeln!(out, "  [{mark}] {:<32} {}", check.name, check.detail);
        }
        out.push_str("\nPlatforms\n");
        for platform in &self.platforms {
            let mark = if platform.ready { "ok  " } else { "--  " };
            let _ = writeln!(out, "  [{mark}] {:<32} {}", platform.platform, platform.detail);
        }
        out
    }

    /// The report as a script reads it.
    ///
    /// # Errors
    ///
    /// Serialization failure, which cannot happen for this shape but is
    /// reported rather than unwrapped.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_covers_every_platform_and_the_toolchains_that_matter() {
        let report = Report::gather();
        let names = report.checks.iter().map(|check| check.name.as_str()).collect::<Vec<_>>();
        for expected in ["rustc", "cargo", "windows sdk", "makeappx", "signtool"] {
            assert!(names.contains(&expected), "{expected} is checked: {names:?}");
        }
        assert_eq!(report.platforms.len(), 7);
        let windows = &report.platforms[0];
        assert_eq!(windows.platform, "windows");
        for platform in &report.platforms[1..] {
            assert!(!platform.ready, "{} has no backend yet", platform.platform);
            assert!(platform.detail.contains("no backend yet"));
        }
        // rustc and cargo are obviously present: they built this test.
        assert!(report.checks[0].ok && report.checks[1].ok);
    }

    #[test]
    fn the_json_report_is_json() {
        let json = Report::gather().to_json().expect("serializable");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["checks"].is_array());
        assert!(parsed["platforms"].is_array());
    }
}
