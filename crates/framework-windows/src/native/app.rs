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
    let mut registry = unsafe { WindowRegistry::new(application) };
    registry.sync()?;

    run_message_loop()?;

    for runtime in registry.runtimes.into_values() {
        if let Some(error) = runtime.error {
            return Err(error);
        }
    }
    Ok(())
}
