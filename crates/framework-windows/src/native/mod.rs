//! The native Win32 window backend: reconciles the platform-independent
//! Rust UI tree against real Win32 windows and controls, and runs the
//! message loop that drives it.

mod app;
mod container;
pub(crate) mod context;
mod input;
mod measure;
mod menu;
mod message_loop;
mod registry;
pub(crate) mod rendering;
mod runtime;
#[cfg(test)]
mod test_support;
mod user_data;
mod util;
pub(crate) mod win32;
pub(crate) mod window_handles;

pub(super) use app::run_application;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::WM_APP;

pub(crate) const WINDOW_CLASS_NAME: &str = "NativeRustFrameworkWindow";
pub(crate) const CONTAINER_CLASS_NAME: &str = "NativeRustFrameworkContainer";
pub(crate) const WM_FRAMEWORK_SCHEDULE: u32 = WM_APP + 1;
pub(crate) const WM_MOUSELEAVE: u32 = 0x02A3;

#[link(name = "user32")]
unsafe extern "system" {
    /// Enables or disables a top-level owner while a modal child is open.
    pub(crate) fn EnableWindow(hwnd: HWND, enable: i32) -> i32;
}
