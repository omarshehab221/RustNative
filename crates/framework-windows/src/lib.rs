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
//! # Getting started
//!
//! ```no_run
//! use framework_core::{Application, Component, Event, Node, Platform, Size, Window};
//! use framework_windows::WindowsPlatform;
//!
//! # struct Greeter;
//! # impl Component for Greeter {
//! #     type Props = ();
//! #     type Message = ();
//! #     fn new((): Self::Props) -> Self { Self }
//! #     fn props(&self) -> &Self::Props { &() }
//! #     fn set_props(&mut self, (): Self::Props) {}
//! #     fn view(&self) -> Node { Node::label("greeting", "Hello") }
//! #     fn update(&mut self, _event: Event) {}
//! # }
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut application =
//!         Application::new(Greeter::new(()), Window::new("Greeter", Size::new(320, 200)));
//!
//!     // Blocks until the last window closes. A failure here carries the
//!     // window and node it concerns — see [`Error`] and [`NativeContext`].
//!     WindowsPlatform::new().run(&mut application)?;
//!     Ok(())
//! }
//! ```
//!
//! Register this backend's OS services so components can reach them through
//! the portable contracts in `framework_core::Services`:
//!
//! ```
//! use std::sync::Arc;
//!
//! use framework_core::Services;
//! # #[cfg(windows)]
//! use framework_windows::{WindowsClipboard, WindowsFileDialogs, WindowsSystem};
//!
//! # #[cfg(windows)]
//! let services = Services::default()
//!     .with_clipboard(Arc::new(WindowsClipboard))
//!     .with_file_dialogs(Arc::new(WindowsFileDialogs))
//!     .with_system(Arc::new(WindowsSystem));
//! # #[cfg(windows)]
//! assert!(services.clipboard().is_some());
//! ```
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
