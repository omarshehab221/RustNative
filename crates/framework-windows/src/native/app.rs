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

    let mut registry = WindowRegistry::new(std::ptr::from_mut::<Application>(application));
    registry.sync()?;

    run_message_loop()?;

    for runtime in registry.runtimes.into_values() {
        if let Some(error) = runtime.error {
            return Err(error);
        }
    }
    Ok(())
}
