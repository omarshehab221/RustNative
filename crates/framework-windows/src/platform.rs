//! The `Platform` entry point this backend exposes to `framework_core`.

#[cfg(windows)]
use framework_core::Capability;
use framework_core::{Application, Platform, PlatformCapabilities};

use crate::Error;

/// The Win32 backend an application hands to `framework_core::Application`
/// to run.
///
/// Carries no state: everything a running application needs
/// (`WindowRegistry`, per-window `Runtime`s, native object registries) is
/// owned by [`Platform::run`]'s own stack frame for exactly as long as the
/// message loop runs — see `native::context`'s module documentation for why
/// that lifetime relationship is what makes the backend's raw-pointer
/// bookkeeping sound.
#[derive(Debug, Default)]
pub struct WindowsPlatform;

impl WindowsPlatform {
    /// Creates the backend. Equivalent to [`Default::default`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
impl Platform for WindowsPlatform {
    type Error = Error;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error> {
        crate::native::run_application(application)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::new([
            Capability::Clipboard,
            Capability::UrlLaunch,
            Capability::MultipleWindows,
            Capability::WindowManagement,
            Capability::FileDialogs,
            Capability::Notifications,
            Capability::Menus,
        ])
    }

    fn native_extension(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(not(windows))]
impl Platform for WindowsPlatform {
    type Error = Error;

    fn run(&mut self, _application: &mut Application) -> Result<(), Self::Error> {
        Err(Error::UnsupportedHost)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::default()
    }

    fn native_extension(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn capabilities_only_advertise_realized_backend_features() {
        let capabilities = WindowsPlatform::new().capabilities();
        assert!(capabilities.supports(Capability::MultipleWindows));
        assert!(capabilities.supports(Capability::WindowManagement));
        assert!(capabilities.supports(Capability::Clipboard));
        assert!(capabilities.supports(Capability::UrlLaunch));
        assert!(capabilities.supports(Capability::FileDialogs));
        assert!(capabilities.supports(Capability::Notifications));
        assert!(capabilities.supports(Capability::Menus));
        // Drag-and-drop, system sharing, and system-appearance change
        // notifications are still only portable contracts (see PLAN.md);
        // this backend does not yet realize them.
        assert!(!capabilities.supports(Capability::DragAndDrop));
        assert!(!capabilities.supports(Capability::SystemShare));
        assert!(!capabilities.supports(Capability::SystemAppearance));
    }
}
