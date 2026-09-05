//! The platform-level error type this backend's `Platform::Error` resolves
//! to.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    #[cfg(windows)]
    WindowsApi {
        operation: &'static str,
        code: u32,
    },
    DuplicateNodeId(String),
    MenuCommandExhausted,
    UnsupportedHost,
    /// A `Component` implementation panicked while running inside a Win32
    /// `WNDPROC` callback. The panic was caught at the FFI boundary (see
    /// `native::message_loop::wndproc_boundary`) before it could unwind into
    /// Win32's own call frames, which is undefined behavior on stable Rust,
    /// and the message loop was asked to exit cleanly instead.
    ComponentPanicked {
        message: String,
    },
}

#[cfg(windows)]
impl Error {
    pub(crate) fn windows_api(operation: &'static str) -> Self {
        Self::WindowsApi {
            operation,
            // SAFETY: `GetLastError` takes no arguments and reads only
            // per-thread state; it is always safe to call, though callers of
            // `Self::windows_api` are relied on to call it immediately after
            // the failing API on the same thread, before any other call
            // overwrites the thread-local error code.
            code: unsafe { windows_sys::Win32::Foundation::GetLastError() },
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(windows)]
            Self::WindowsApi { operation, code } => {
                write!(f, "Windows API call {operation} failed with error code {code}")
            }
            Self::DuplicateNodeId(id) => write!(f, "duplicate UI node id: {id}"),
            Self::MenuCommandExhausted => f.write_str("native menu command ID space exhausted"),
            Self::UnsupportedHost => f.write_str("framework-windows is only runnable on Windows"),
            Self::ComponentPanicked { message } => {
                write!(f, "a component panicked inside the native message loop: {message}")
            }
        }
    }
}

impl std::error::Error for Error {}
