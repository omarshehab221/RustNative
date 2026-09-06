//! The `Platform::run` entry point for the native Win32 backend: registers
//! window classes, brings up every window the `Application` currently wants
//! open, and runs the message loop until the process quits.

use framework_core::Application;

use super::message_loop::{register_window_classes, run_message_loop};
use super::runtime::WindowRegistry;
use super::util::module_instance;
use crate::Error;

pub(crate) fn run_application(application: &mut Application) -> Result<(), Error> {
    let instance = module_instance();
    register_window_classes(instance)?;

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
