//! `rf`, the Rust Native command.
//!
//! ```text
//! rf new <name> [--path DIR] [--framework-path DIR]
//! rf build <platform> [--release]
//! rf run   <platform> [--release]
//! rf check [platform]
//! rf test  [-- cargo test arguments]
//! rf doctor [--json]
//! ```
//!
//! `rf` **orchestrates** the toolchains rather than replacing them: a build
//! is Cargo's build, with Cargo's own output, and the Windows SDK tools are
//! found where they are installed rather than bundled. Platforms whose
//! backend the roadmap has not reached are recognized and refused by name,
//! with the milestone that will bring them — never quietly built for
//! Windows instead.
//!
//! A project is a folder with an `rf.toml` (see [`config`]), which holds
//! the identity the application's saved state, single-instance mutex, and
//! package are keyed by.

#![deny(missing_docs)]

mod cli;
mod config;
mod doctor;
mod error;
mod platform;
mod project;
mod toolchain;

use std::process::ExitCode;

use clap::Parser as _;

fn main() -> ExitCode {
    match cli::Cli::parse().run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
