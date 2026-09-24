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
    /// Generate a binding from an interface description (`.ril`): the C
    /// header, the C# bindings, or the Rust implementation shims.
    Bindgen {
        /// The `.ril` file.
        file: PathBuf,
        /// The language to generate.
        #[arg(long, value_enum)]
        lang: BindgenLanguage,
        /// Where to write it (standard output if omitted).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Browse the application's previews in the preview catalogue — every
    /// preview across its configurations — or, with `--headless`, run them
    /// as golden tests (`PLAN.md` Milestone 43).
    Preview {
        /// The preview to open first.
        name: Option<String>,
        /// Run the previews as golden tests on the headless backend instead.
        #[arg(long)]
        headless: bool,
    },
    /// Measure the framework's budget scenarios for a target against
    /// `budgets/<target>.toml`, writing `target/budget-report.json`
    /// (`PLAN.md` Milestone 42). Run in the framework's repository.
    Bench {
        /// The target whose budget file to measure against.
        #[arg(long, value_enum, default_value = "windows")]
        target: crate::bench::BenchTarget,
        /// Fail on a measurement over budget, or one the budget file does
        /// not declare.
        #[arg(long)]
        check: bool,
        /// Pin the scenarios to one core: the low-end reference profile.
        #[arg(long)]
        low_end: bool,
        /// Also time a clean and an incremental build.
        #[arg(long)]
        build_times: bool,
    },
    /// Inspect a running application: its tree, components and state,
    /// layout and style explanations, trace, tasks, capabilities; the
    /// overlay; recording (`PLAN.md` Milestone 44).
    Inspect {
        /// Where the application is.
        #[command(flatten)]
        target: crate::inspect::Target,
        /// What to ask.
        #[command(subcommand)]
        question: crate::inspect::Question,
    },
    /// Print the builder form a file's markup lowers to — a `.rsx` file, or
    /// the `rsx!` calls in a `.rs` file — or, with `--classes`/`--styles`,
    /// the declarations and typed values a class string or declaration
    /// block lowers to, against the project's `app.css`.
    Expand {
        /// The file.
        #[arg(required_unless_present_any = ["classes", "styles"])]
        file: Option<PathBuf>,
        /// A class string to expand.
        #[arg(long, conflicts_with = "file")]
        classes: Option<String>,
        /// A declaration block to expand.
        #[arg(long, conflicts_with_all = ["file", "classes"])]
        styles: Option<String>,
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
        /// Profile-guided: instrument, run a scripted startup, rebuild with
        /// the profile (needs `rustup component add llvm-tools`).
        #[arg(long, requires = "release")]
        pgo: bool,
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
                let parent = path.unwrap_or_else(|| here.clone());
                // A relative checkout path names a folder from here, not from
                // the new project, where Cargo will read it.
                let framework = framework_path.map_or_else(
                    || FrameworkSource::Published(FRAMEWORK_VERSION.to_owned()),
                    |path| {
                        FrameworkSource::Path(if path.is_absolute() {
                            path
                        } else {
                            here.join(path)
                        })
                    },
                );
                let root = create(&parent, &name, &framework, syntax)?;
                println!("Created {}", root.display());
                println!("  cd {name}");
                println!("  rustnative run windows");
                Ok(())
            }
            Command::Build { platform, release, pgo } => {
                if pgo {
                    if platform.backend().is_none() {
                        return Err(Error::NoBackend {
                            platform,
                            milestone: platform.planned_milestone(),
                        });
                    }
                    crate::pgo::build(&Project::find(&here)?)
                } else {
                    cargo_for(platform, &here, "build", release, &[])
                }
            }
            Command::Run { platform, release } => cargo_for(platform, &here, "run", release, &[]),
            Command::Check { platform } => cargo_for(platform, &here, "check", false, &[]),
            Command::Test { arguments } => {
                let project = Project::find(&here)?;
                let mut command = vec!["test".to_owned()];
                command.extend(arguments);
                crate::diagnostics::run_cargo(&project.root, &command)
            }
            Command::Bindgen { file, lang, out } => {
                let source = std::fs::read_to_string(&file).map_err(|cause| Error::Io {
                    what: format!("read {}", file.display()),
                    cause,
                })?;
                let idl = framework_interop::parse_idl(&source)
                    .map_err(|error| Error::Usage(format!("{}:{error}", file.display())))?;
                let text = match lang {
                    BindgenLanguage::C => framework_interop::generate::c(&idl),
                    BindgenLanguage::Csharp => framework_interop::generate::csharp(&idl),
                    BindgenLanguage::Rust => framework_interop::generate::rust(&idl),
                };
                match out {
                    Some(path) => std::fs::write(&path, text).map_err(|cause| Error::Io {
                        what: format!("write {}", path.display()),
                        cause,
                    })?,
                    None => print!("{text}"),
                }
                Ok(())
            }
            Command::Inspect { target, question } => crate::inspect::run(&target, question),
            Command::Preview { name, headless } => {
                let project = Project::find(&here)?;
                if headless {
                    let arguments = ["test".to_owned(), "--test".to_owned(), "previews".to_owned()];
                    return crate::diagnostics::run_cargo(&project.root, &arguments);
                }
                // The application's own `main` shows the catalogue when asked.
                let status = std::process::Command::new(
                    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
                )
                .current_dir(&project.root)
                .arg("run")
                .env(framework_core::preview::PREVIEW_VARIABLE, name.unwrap_or_default())
                .status()
                .map_err(|cause| Error::ToolMissing {
                    tool: "cargo",
                    hint: "install the Rust toolchain from https://rustup.rs".into(),
                    cause: Some(cause.to_string()),
                })?;
                if status.success() {
                    Ok(())
                } else {
                    Err(Error::ToolFailed { tool: "cargo", code: status.code() })
                }
            }
            Command::Bench { target, check, low_end, build_times } => {
                crate::bench::run(&here, target, check, low_end, build_times)
            }
            Command::Expand { file, classes, styles } => {
                let text = match (file, classes, styles) {
                    (Some(file), _, _) => crate::markup::expand_file(&file)?,
                    (None, Some(classes), _) => crate::markup::expand_style(&here, &classes, true)?,
                    (None, None, Some(styles)) => {
                        crate::markup::expand_style(&here, &styles, false)?
                    }
                    (None, None, None) => {
                        return Err(Error::Usage(
                            "name a file, `--classes`, or `--styles`".to_owned(),
                        ));
                    }
                };
                print!("{text}");
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

/// The languages `rustnative bindgen` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum BindgenLanguage {
    /// A C header.
    C,
    /// C# P/Invoke bindings.
    Csharp,
    /// The Rust implementation shims.
    Rust,
}
