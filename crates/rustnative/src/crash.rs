//! `rustnative crash` (`PLAN.md` Milestone 51): the crash reports an
//! application left on this machine.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::project::Project;

/// `rustnative crash`.
#[derive(Debug, clap::Subcommand)]
pub enum CrashCommand {
    /// List the reports, newest first.
    List,
    /// Show one report: the message, the backtrace, and the tree at failure.
    Show {
        /// The report's id (from `list`).
        id: String,
    },
}

/// Where the application's reports are (`%LOCALAPPDATA%\<id>\crashes`).
fn directory(id: &str) -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
    base.join(id).join("crashes")
}

fn reports(directory: &Path) -> Vec<Value> {
    let mut reports: Vec<Value> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
        .filter_map(|entry| serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok())
        .collect();
    reports.sort_by_key(|report| std::cmp::Reverse(report["time"].as_u64().unwrap_or(0)));
    reports
}

/// Runs a `crash` command.
///
/// # Errors
///
/// The project cannot be read, or there is no such report.
pub fn run(here: &Path, command: &CrashCommand) -> Result<()> {
    let project = Project::find(here)?;
    let directory = directory(&project.config.app.id);
    let reports = reports(&directory);
    match command {
        CrashCommand::List => {
            if reports.is_empty() {
                println!("crash: no reports in {}", directory.display());
            }
            for report in &reports {
                println!(
                    "{}  {}  {}",
                    report["id"].as_str().unwrap_or_default(),
                    report["app_version"].as_str().unwrap_or_default(),
                    report["message"].as_str().unwrap_or_default()
                );
            }
            Ok(())
        }
        CrashCommand::Show { id } => {
            let report =
                reports.iter().find(|report| report["id"] == id.as_str()).ok_or_else(|| {
                    Error::Usage(format!("no crash report {id} in {}", directory.display()))
                })?;
            println!("{}", report["message"].as_str().unwrap_or_default());
            println!(
                "version {}  {}",
                report["app_version"].as_str().unwrap_or_default(),
                report["os"].as_str().unwrap_or_default()
            );
            if let Some(dump) = report["dump"].as_str() {
                println!("minidump {dump}");
            }
            println!("\n{}", report["backtrace"].as_str().unwrap_or_default());
            if let Some(tree) = report["tree"].as_str() {
                let pretty = serde_json::from_str::<Value>(tree)
                    .ok()
                    .and_then(|tree| serde_json::to_string_pretty(&tree).ok())
                    .unwrap_or_else(|| tree.to_owned());
                println!("\nThe tree when it failed:\n{pretty}");
            }
            Ok(())
        }
    }
}
