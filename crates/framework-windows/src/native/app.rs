//! The `Platform::run` entry point for the native Win32 backend: registers
//! window classes, brings up every window the `Application` currently wants
//! open, and runs the message loop until the process quits.

use framework_core::Application;

use super::message_loop::{register_window_classes, run_message_loop};
use super::runtime::WindowRegistry;
use super::util::module_instance;
use super::win32::best_effort;
use crate::Error;

/// OLE initialized on the UI thread for as long as this value lives.
///
/// Drag-and-drop registration (`RegisterDragDrop`) requires the calling
/// thread to have entered a single-threaded apartment through
/// `OleInitialize` — plain `CoInitializeEx` is not enough. It also makes the
/// COM objects the accessibility bridge creates on this thread available,
/// which previously degraded to a no-op outside of the dialog threads.
///
/// Initialization failing (most likely because the application already
/// entered a multithreaded apartment on this thread) is not fatal: windows
/// still work, they just do not accept drops.
pub(crate) struct OleApartment {
    initialized: bool,
}

impl OleApartment {
    pub(crate) fn enter() -> Self {
        // SAFETY: a null reserved pointer is the documented argument; the
        // call is made on the UI thread before any window exists.
        let initialized = unsafe { windows::Win32::System::Ole::OleInitialize(None) }.is_ok();
        best_effort(initialized, "OleInitialize", "windows on this thread do not accept drops");
        Self { initialized }
    }
}

impl Drop for OleApartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: pairs the successful `OleInitialize` in `enter`, on the
            // same thread (this type is `!Send` through its use sites: it is
            // created and dropped within one function or one harness).
            unsafe { windows::Win32::System::Ole::OleUninitialize() };
        }
    }
}

pub(crate) fn run_application(application: &mut Application) -> Result<(), Error> {
    let instance = module_instance();
    register_window_classes(instance)?;
    // Declared before the registry so it is dropped after it: OLE must stay
    // initialized until every window has revoked its drop target.
    let _ole = OleApartment::enter();

    // SAFETY: `application` is borrowed for the whole of this function,
    // and `registry` — along with every `Runtime` it creates, each of which
    // captures a copy of the same borrow — is dropped at the end of it,
    // after `run_message_loop` has returned. That is exactly point 1 of
    // `native::context`'s module documentation, which `WindowRegistry::new`
    // requires its caller to establish.
    //
    // The `Box` is load-bearing, not a convenience. Each `Runtime` the
    // registry creates holds a `HostRef<WindowRegistry>` pointing at the
    // registry's address (so it can reconcile the window set after a
    // dispatch), and those references are captured during `sync`. Behind a
    // `Box`, the registry's address is fixed at allocation and stays valid
    // however the owning handle is later moved — the invariant becomes
    // structural rather than a fact about this one function's body.
    let mut registry = Box::new(unsafe { WindowRegistry::new(application) });
    registry.sync()?;

    run_message_loop()?;

    for runtime in registry.runtimes.drain().map(|(_, runtime)| runtime) {
        if let Some(error) = runtime.error {
            return Err(error);
        }
    }
    Ok(())
}
