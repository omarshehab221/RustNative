//! Windows platform backend.
//!
//! The backend owns native Win32 objects and reconciles them against the
//! platform-independent Rust UI tree. Containers are native child windows,
//! which makes the Rust tree hierarchy correspond to a real Win32 hierarchy.
//!
//! Module map (private modules; the public API is re-exported flat from
//! this crate root, listed below):
//! - `error`: the [`Error`] type `Platform::Error` resolves to, plus the
//!   [`NativeContext`]/[`Win32Category`] it carries.
//! - `platform`: [`WindowsPlatform`], the `Platform` trait implementation.
//! - `ffi`: tiny string/memory helpers shared across services and the
//!   native backend.
//! - `services`: clipboard, shell/system, file dialogs, and tray
//!   notifications — the non-window OS integrations.
//! - `native`: the Win32 window backend proper (tree reconciliation,
//!   layout, the message loop, and the window procedures).
//!
//! # Documentation coverage
//!
//! Every public item — fields and enum variants included — carries a doc
//! comment, enforced by `#![deny(missing_docs)]` below. `framework-core` has
//! had this since the standards audit's P2.23 pass; this crate did not, even
//! though its public surface is the one an application actually touches to
//! stand a backend up (`WindowsPlatform`, `Error`, the service types). The
//! lint is `deny` rather than `warn` so the coverage cannot silently regress.
#![deny(missing_docs)]

mod error;
mod ffi;
#[cfg(windows)]
mod native;
mod platform;
mod services;

pub use error::{Error, NativeContext, Win32Category};
pub use platform::WindowsPlatform;
#[cfg(windows)]
pub use services::clipboard::WindowsClipboard;
#[cfg(windows)]
pub use services::dialogs::WindowsFileDialogs;
#[cfg(windows)]
pub use services::system::WindowsSystem;
