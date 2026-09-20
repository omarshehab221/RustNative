//! What can go wrong, and what the shell learns from it.
//!
//! Exit codes are part of the interface: a script that runs `rustnative build
//! macos` can tell "no backend for that platform yet" (3) from "the build
//! failed" (1) without reading the message.

use std::fmt;

use crate::config::ConfigError;
use crate::platform::Platform;

/// The result of anything `rustnative` does.
pub type Result<T> = std::result::Result<T, Error>;

/// Something `rustnative` could not do.
#[derive(Debug)]
pub enum Error {
    /// The project's `rustnative.toml` is missing or wrong.
    Config(ConfigError),
    /// A file could not be read or written.
    Io {
        /// What was being done ("create the project folder").
        what: String,
        /// Why it failed.
        cause: std::io::Error,
    },
    /// The platform is known but has no backend yet.
    NoBackend {
        /// The platform asked for.
        platform: Platform,
        /// The milestone that will bring it, if it is numbered.
        milestone: Option<u32>,
    },
    /// A tool `rustnative` needs is not installed.
    ToolMissing {
        /// The tool's name.
        tool: &'static str,
        /// How to get it.
        hint: String,
        /// What the operating system said, if anything.
        cause: Option<String>,
    },
    /// A tool ran and failed.
    ToolFailed {
        /// The tool's name.
        tool: &'static str,
        /// Its exit code, if it had one.
        code: Option<i32>,
    },
    /// The command cannot be used this way.
    Usage(String),
}

impl Error {
    /// The process exit code this error produces.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            // A usage mistake, like clap's own.
            Self::Usage(_) => 2,
            // A platform the roadmap has not reached: distinct, so a script
            // can tell "not yet" from "it broke".
            Self::NoBackend { .. } => 3,
            // A missing toolchain is the person's to install.
            Self::ToolMissing { .. } => 4,
            Self::Config(_) | Self::Io { .. } | Self::ToolFailed { .. } => 1,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "{error}"),
            Self::Io { what, cause } => write!(f, "could not {what}: {cause}"),
            Self::NoBackend { platform, milestone } => {
                write!(f, "no backend for {platform} yet")?;
                match milestone {
                    Some(milestone) => {
                        write!(f, " — see PLAN.md, Milestone {milestone}")
                    }
                    None => write!(f, " — see PLAN.md's web platform roadmap"),
                }
            }
            Self::ToolMissing { tool, hint, cause } => {
                write!(f, "{tool} is not available: {hint}")?;
                match cause {
                    Some(cause) => write!(f, " ({cause})"),
                    None => Ok(()),
                }
            }
            Self::ToolFailed { tool, code } => match code {
                Some(code) => write!(f, "{tool} failed with exit code {code}"),
                None => write!(f, "{tool} was stopped by a signal"),
            },
            Self::Usage(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<ConfigError> for Error {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_failure_has_its_own_exit_code() {
        let no_backend = Error::NoBackend { platform: Platform::Macos, milestone: Some(33) };
        assert_eq!(no_backend.exit_code(), 3);
        assert!(no_backend.to_string().contains("Milestone 33"));
        assert_eq!(Error::Usage("nope".to_owned()).exit_code(), 2);
        assert_eq!(
            Error::ToolMissing { tool: "cargo", hint: String::new(), cause: None }.exit_code(),
            4
        );
        assert_eq!(Error::ToolFailed { tool: "cargo", code: Some(101) }.exit_code(), 1);
    }
}
