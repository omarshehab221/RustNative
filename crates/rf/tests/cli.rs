//! Milestone 31 acceptance tests: the `rf` binary, run as a person runs
//! it.
//!
//! Every test here starts the real executable in a real folder and checks
//! its exit code and output. The heaviest one generates a project and
//! compiles it with Cargo, which is the only way to know the templates
//! produce something that builds.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The `rf` binary Cargo built for this test.
fn rf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rf"))
}

/// This workspace's root, which generated projects depend on by path.
fn workspace() -> PathBuf {
    // `crates/rf` -> the workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// A fresh folder for one test.
fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("rf-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch folder");
    directory
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Creates a project in a scratch folder, depending on this workspace.
fn new_project(name: &str) -> PathBuf {
    let parent = scratch(name);
    let output = rf()
        .args(["new", name, "--path"])
        .arg(&parent)
        .arg("--framework-path")
        .arg(workspace())
        .output()
        .expect("rf runs");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Created"));
    parent.join(name)
}

#[test]
fn a_generated_project_compiles() {
    let project = new_project("compiles");
    // The workspace's own target folder: the framework and its dependencies
    // are already built there, so this checks the generated code rather
    // than spending minutes rebuilding the world. `--offline` keeps the
    // test from depending on the network.
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(&project)
        .args(["check", "--offline"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("cargo runs");
    assert!(output.status.success(), "the generated project must compile:\n{}", stderr(&output));
}

#[test]
fn a_new_project_has_a_valid_config_and_is_found_from_inside_it() {
    let project = new_project("valid-config");
    let config = std::fs::read_to_string(project.join("rf.toml")).unwrap();
    assert!(config.contains("name = \"valid-config\""), "{config}");
    assert!(config.contains("id = \"com.example.validconfig\""), "{config}");

    // `rf check` finds the project from a subfolder, and gets as far as
    // running Cargo (which is what "found the project" looks like).
    let output = rf()
        .current_dir(project.join("src"))
        .args(["check", "windows"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rf runs");
    assert!(stdout(&output).contains("check: valid-config for windows"), "{}", stdout(&output));
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn every_platform_without_a_backend_is_refused_by_name() {
    let project = new_project("no-backend");
    let expected = [
        ("macos", "Milestone 33"),
        ("linux", "Milestone 34"),
        ("android", "Milestone 35"),
        ("ios", "Milestone 36"),
        ("embedded", "Milestone 37"),
        ("web", "web platform roadmap"),
    ];
    for (platform, mention) in expected {
        let output =
            rf().current_dir(&project).args(["build", platform]).output().expect("rf runs");
        assert_eq!(output.status.code(), Some(3), "{platform}: {}", stderr(&output));
        let message = stderr(&output);
        assert!(message.contains(&format!("no backend for {platform} yet")), "{message}");
        assert!(message.contains(mention), "{message}");
    }
}

#[test]
fn an_unknown_platform_is_a_usage_error() {
    let project = new_project("unknown-platform");
    let output = rf().current_dir(&project).args(["build", "atari"]).output().expect("rf runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("atari"), "{}", stderr(&output));
}

#[test]
fn a_broken_config_names_the_field() {
    let project = new_project("broken-config");
    let config = std::fs::read_to_string(project.join("rf.toml")).unwrap();
    std::fs::write(
        project.join("rf.toml"),
        config.replace("version = \"0.1.0\"", "version = \"1\""),
    )
    .unwrap();

    let output = rf().current_dir(&project).args(["build", "windows"]).output().expect("rf runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("app.version"), "{}", stderr(&output));
}

#[test]
fn outside_a_project_the_error_says_how_to_make_one() {
    let empty = scratch("outside");
    let output = rf().current_dir(&empty).args(["build", "windows"]).output().expect("rf runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("rf new"), "{}", stderr(&output));
}

/// Windows-only: what `doctor` reports about the MSVC toolchain and the
/// SDK is only true on a machine that has them, and on Linux `doctor`
/// correctly reports the opposite (and exits non-zero for it).
#[cfg(windows)]
#[test]
fn doctor_reports_this_machine_as_json() {
    let output = rf().args(["doctor", "--json"]).output().expect("rf runs");
    let report: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("valid JSON on stdout");
    let checks = report["checks"].as_array().expect("checks");
    let named = |name: &str| {
        checks
            .iter()
            .find(|check| check["name"] == name)
            .unwrap_or_else(|| panic!("{name} is checked"))
            .clone()
    };
    assert_eq!(named("rustc")["ok"], serde_json::Value::Bool(true), "rustc built this test");
    assert_eq!(named("cargo")["ok"], serde_json::Value::Bool(true));

    // This machine built `framework-windows`, so the MSVC toolchain and the
    // SDK are here; `doctor` must agree, and say where they are.
    for name in ["visual studio (c++ build tools)", "windows sdk", "rc", "mt"] {
        assert_eq!(named(name)["ok"], serde_json::Value::Bool(true), "{name}: {}", named(name));
        assert!(!named(name)["detail"].as_str().unwrap_or_default().is_empty());
    }

    let platforms = report["platforms"].as_array().expect("platforms");
    let windows = platforms.iter().find(|entry| entry["platform"] == "windows").expect("windows");
    assert_eq!(windows["ready"], serde_json::Value::Bool(true));
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn doctor_without_json_is_a_readable_table() {
    let output = rf().arg("doctor").output().expect("rf runs");
    let text = stdout(&output);
    assert!(text.contains("Toolchains"), "{text}");
    assert!(text.contains("Platforms"), "{text}");
    assert!(text.contains("no backend yet — Milestone 33"), "{text}");
}

#[test]
fn test_passes_its_arguments_through_to_cargo() {
    let project = new_project("passthrough");
    // A filter that matches nothing still succeeds; a nonsense *flag* does
    // not. Both prove the arguments reached Cargo.
    let ok = rf()
        .current_dir(&project)
        .args(["test", "--offline", "no-such-test"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rf runs");
    assert!(ok.status.success(), "{}", stderr(&ok));

    let bad = rf()
        .current_dir(&project)
        .args(["test", "--definitely-not-a-cargo-flag"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rf runs");
    assert_eq!(bad.status.code(), Some(1));
    assert!(stderr(&bad).contains("cargo failed"), "{}", stderr(&bad));
}

#[test]
fn creating_over_an_existing_project_is_refused() {
    let project = new_project("twice");
    let parent: &Path = project.parent().unwrap();
    let output = rf()
        .args(["new", "twice", "--path"])
        .arg(parent)
        .arg("--framework-path")
        .arg(workspace())
        .output()
        .expect("rf runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("already exists"), "{}", stderr(&output));
}

/// Windows-only: the executable's name, and the linker that produces it.
#[cfg(windows)]
#[test]
fn building_a_generated_project_produces_an_executable() {
    let project = new_project("builds");
    let target = workspace().join("target");
    let output = rf()
        .current_dir(&project)
        .args(["build", "windows"])
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .expect("rf runs");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("build: builds for windows"), "{}", stdout(&output));
    assert!(
        target.join("debug").join("builds.exe").is_file(),
        "an executable at {}",
        target.join("debug").join("builds.exe").display()
    );
}
