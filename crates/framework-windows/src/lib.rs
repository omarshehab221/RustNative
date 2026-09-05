//! Windows platform backend.
//!
//! The backend owns native Win32 objects and reconciles them against the
//! platform-independent Rust UI tree. Containers are native child windows,
//! which makes the Rust tree hierarchy correspond to a real Win32 hierarchy.
//!
//! Module map:
//! - [`error`]: the `Error` type `Platform::Error` resolves to.
//! - [`platform`]: `WindowsPlatform`, the `Platform` trait implementation.
//! - [`ffi`]: tiny string/memory helpers shared across services and the
//!   native backend.
//! - [`services`]: clipboard, shell/system, file dialogs, and tray
//!   notifications — the non-window OS integrations.
//! - [`native`]: the Win32 window backend proper (tree reconciliation,
//!   layout, the message loop, and the window procedures).

mod error;
mod ffi;
#[cfg(windows)]
mod native;
mod platform;
mod services;

pub use error::Error;
pub use platform::WindowsPlatform;
#[cfg(windows)]
pub use services::clipboard::WindowsClipboard;
#[cfg(windows)]
pub use services::dialogs::WindowsFileDialogs;
#[cfg(windows)]
pub use services::system::WindowsSystem;
