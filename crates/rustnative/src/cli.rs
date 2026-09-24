//! The command line itself, and what each command does.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::doctor::Report;
use crate::error::{Error, Result};
use crate::platform::Platform;
use crate::project::{FrameworkSource, Project, create};

/// The version of the framework a generated project depends on when it is
/// not pointed at a checkout.
const FRAMEWORK_VERSION: &str = "0.1";

/// Create, build, run, and diagnose Rust Native applications.
#[derive(Debug, Parser)]
#[command(name = "rustnative", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// What `rustnative` was asked to do.
#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new application.
    New {
        /// The project's name: a folder, a Cargo package, an executable.
        name: String,
        /// Where to create it (the current folder by default).
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,
        /// Depend on a checkout of the framework rather than on published
        /// versions — what to use while working on the framework itself.
        #[arg(long, value_name = "DIR")]
        framework_path: Option<PathBuf>,
        /// Which syntax the project is written in. There is no default:
        /// neither is the one a developer should prefer (`PLAN.md` 2.9),
        /// and both templates are the same application.
        #[arg(long, value_enum)]
        syntax: crate::project::Syntax,
    },
    /// Print the builder form a file's markup lowers to — a `.rsx` file, or
    /// the `rsx!` calls in a `.rs` file.
    Expand {
        /// The file.
        file: PathBuf,
    },
    /// Format `.rsx` files (Rust and markup together) and the `rsx!` calls
    /// in `.rs` files — every such file in the project if none is named.
    Fmt {
        /// The files to format.
        files: Vec<PathBuf>,
        /// Report files that would change, and fail, instead of writing.
        #[arg(long)]
        check: bool,
    },
    /// Serve the language server protocol for `.rsx` files, forwarding to
    /// `rust-analyzer` over the lowered files.
    Lsp {
        /// The language server to forward to (a program and its arguments).
        #[arg(long, default_value = "rust-analyzer")]
        server: String,
    },
    /// A stand-in language server the `lsp` tests forward to.
    #[command(hide = true, name = "__echo-lsp")]
    EchoLsp,
    /// Build the application.
    Build {
        /// Which platform to build for.
        platform: Platform,
        /// Build with optimizations.
        #[arg(long)]
        release: bool,
    },
    /// Build and run the application.
    Run {
        /// Which platform to run on.
        platform: Platform,
        /// Build with optimizations.
        #[arg(long)]
        release: bool,
    },
    /// Check the application compiles, without building it fully.
    Check {
        /// Which platform to check for.
        #[arg(default_value = "windows")]
        platform: Platform,
    },
    /// Run the application's tests.
    Test {
        /// Arguments passed through to `cargo test`, flags included
        /// (`rustnative test --offline -- --nocapture`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Build the application and package it for distribution.
    Package {
        /// Which platform to package for.
        platform: Platform,
        /// Which package to produce.
        #[arg(long, value_enum, default_value = "all")]
        format: crate::package::Format,
        /// A `.pfx` certificate to sign the MSIX with.
        #[arg(long, value_name = "CERT")]
        sign: Option<PathBuf>,
        /// The environment variable holding the certificate's password.
        #[arg(long, value_name = "VAR", requires = "sign")]
        password_env: Option<String>,
    },
    /// Report what this machine can build.
    Doctor {
        /// Print the findings as JSON.
        #[arg(long)]
        json: bool,
    },
}

impl Cli {
    /// Runs the command.
    ///
    /// # Errors
    ///
    /// Whatever the command could not do; see [`Error::exit_code`].
    pub fn run(self) -> Result<()> {
        let here = std::env::current_dir()
            .map_err(|cause| Error::Io { what: "find the current folder".to_owned(), cause })?;
        match self.command {
            Command::New { name, path, framework_path, syntax } => {
                let parent = path.unwrap_or(here);
                let framework = framework_path.map_or_else(
                    || FrameworkSource::Published(FRAMEWORK_VERSION.to_owned()),
                    FrameworkSource::Path,
                );
                let root = create(&parent, &name, &framework, syntax)?;
                println!("Created {}", root.display());
                println!("  cd {name}");
                println!("  rustnative run windows");
                Ok(())
            }
            Command::Build { platform, release } => {
                cargo_for(platform, &here, "build", release, &[])
            }
            Command::Run { platform, release } => cargo_for(platform, &here, "run", release, &[]),
            Command::Check { platform } => cargo_for(platform, &here, "check", false, &[]),
            Command::Test { arguments } => {
                let project = Project::find(&here)?;
                let mut command = vec!["test".to_owned()];
                command.extend(arguments);
                crate::diagnostics::run_cargo(&project.root, &command)
            }
            Command::Expand { file } => {
                print!("{}", crate::markup::expand_file(&file)?);
                Ok(())
            }
            Command::Fmt { files, check } => {
                let files = if files.is_empty() {
                    let project = Project::find(&here)?;
                    crate::markup::project_files(&project.root.join("src"))
                } else {
                    files
                };
                let mut unformatted = Vec::new();
                for file in &files {
                    let formatted = crate::markup::format_file(file)?;
                    let current = std::fs::read_to_string(file)
                        .map_err(|cause| Error::Io {
                            what: format!("read {}", file.display()),
                            cause,
                        })?
                        .replace("\r\n", "\n");
                    if formatted == current {
                        continue;
                    }
                    if check {
                        unformatted.push(file.display().to_string());
                    } else {
                        std::fs::write(file, formatted).map_err(|cause| Error::Io {
                            what: format!("write {}", file.display()),
                            cause,
                        })?;
                        println!("Formatted {}", file.display());
                    }
                }
                if unformatted.is_empty() {
                    Ok(())
                } else {
                    Err(Error::Usage(format!("not formatted:\n  {}", unformatted.join("\n  "))))
                }
            }
            Command::Lsp { server } => crate::lsp::serve(&server),
            Command::EchoLsp => crate::lsp::echo_server(),
            Command::Package { platform, format, sign, password_env } => {
                if platform.backend().is_none() {
                    return Err(Error::NoBackend {
                        platform,
                        milestone: platform.planned_milestone(),
                    });
                }
                let project = Project::find(&here)?;
                let signing = sign
                    .map(|certificate| crate::package::sign::Signing { certificate, password_env });
                let produced = crate::package::package(
                    &project.root,
                    &project.config,
                    format,
                    signing.as_ref(),
                )?;
                for path in produced {
                    println!("Packaged {}", path.display());
                }
                Ok(())
            }
            Command::Doctor { json } => {
                let report = Report::gather();
                if json {
                    let text = report.to_json().map_err(|cause| Error::Io {
                        what: "write the report".to_owned(),
                        cause: std::io::Error::other(cause.to_string()),
                    })?;
                    println!("{text}");
                } else {
                    print!("{}", report.to_text());
                }
                // A machine that cannot build is a failure a script should
                // see, not just read about.
                if report.is_healthy() {
                    Ok(())
                } else {
                    Err(Error::Usage(
                        "some toolchains are missing; see the report above".to_owned(),
                    ))
                }
            }
        }
    }
}

/// Runs one Cargo subcommand for `platform`, refusing the platforms that
/// have no backend.
fn cargo_for(
    platform: Platform,
    here: &std::path::Path,
    subcommand: &str,
    release: bool,
    extra: &[&str],
) -> Result<()> {
    if platform.backend().is_none() {
        return Err(Error::NoBackend { platform, milestone: platform.planned_milestone() });
    }
    let project = Project::find(here)?;
    println!("{subcommand}: {} for {platform}", project.config.app.display_name);
    let mut arguments = vec![subcommand.to_owned()];
    if release {
        arguments.push("--release".to_owned());
    }
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    // Structured diagnostics, so positions in lowered `.rsx` files are
    // reported in the `.rsx` file (see `diagnostics`).
    crate::diagnostics::run_cargo(&project.root, &arguments)
}
