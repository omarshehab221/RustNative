//! The `Platform` entry point this backend exposes to `framework_core`.

#[cfg(windows)]
use framework_core::Capability;
use framework_core::{Application, Platform, PlatformCapabilities};

use crate::Error;

/// The Win32 backend an application hands to `framework_core::Application`
/// to run.
///
/// Carries only configuration: everything a running application needs
/// (`WindowRegistry`, per-window `Runtime`s, native object registries) is
/// owned by [`Platform::run`]'s own stack frame for exactly as long as the
/// message loop runs — see `native::context`'s module documentation for why
/// that lifetime relationship is what makes the backend's raw-pointer
/// bookkeeping sound.
#[derive(Debug, Default, Clone)]
pub struct WindowsPlatform {
    app_id: Option<String>,
}

impl WindowsPlatform {
    /// Creates the backend. Equivalent to [`Default::default`].
    #[must_use]
    pub const fn new() -> Self {
        Self { app_id: None }
    }

    /// Identifies the application, which makes it **single-instance**.
    ///
    /// With an id, a second launch does not open a second copy: it hands
    /// the URL it was launched with (the first `scheme://…` command-line
    /// argument, which is how Windows launches an application for a
    /// protocol it handles) to the running instance as
    /// [`framework_core::Event::DeepLink`], brings that instance to the
    /// front, and exits. The first launch's own URL, if any, is delivered
    /// the same way once its windows exist.
    ///
    /// Use the same id as [`crate::FileStateStore::for_app`]; a reverse
    /// domain name (`com.example.notes`) is conventional.
    #[must_use]
    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }

    /// The application id, if one was set.
    #[must_use]
    pub fn app_id(&self) -> Option<&str> {
        self.app_id.as_deref()
    }
}

#[cfg(windows)]
impl Platform for WindowsPlatform {
    type Error = Error;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error> {
        crate::native::run_application(
            application,
            self.app_id.as_deref(),
            crate::native::single_instance::launch_url(std::env::args()),
        )
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
            Capability::DragAndDrop,
            Capability::Touch,
            Capability::Pen,
            Capability::Gamepad,
            Capability::Ime,
            Capability::Animations,
            Capability::ReducedMotionPreference,
            Capability::CustomDrawing,
            Capability::NativeSurfaces,
            Capability::StatePersistence,
            Capability::DeepLinks,
            Capability::Lifecycle,
            // Milestone 39: cursors per node, hover, command shortcuts,
            // host-mirrored right-to-left, host traits in the environment
            // (which includes following the system appearance), and the
            // consent-store permission states.
            Capability::Cursors,
            Capability::Hover,
            Capability::CommandShortcuts,
            Capability::RightToLeft,
            Capability::HostTraits,
            Capability::SystemAppearance,
            Capability::Permissions,
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
        // Milestone 25: OLE drop target, `WM_POINTER` touch and pen,
        // `XInput` controllers, and IMM32 composition.
        assert!(capabilities.supports(Capability::DragAndDrop));
        assert!(capabilities.supports(Capability::Touch));
        assert!(capabilities.supports(Capability::Pen));
        assert!(capabilities.supports(Capability::Gamepad));
        assert!(capabilities.supports(Capability::Ime));
        // Milestone 27: frame-driven native property animation, and the
        // system's own reduced-motion setting.
        assert!(capabilities.supports(Capability::Animations));
        assert!(capabilities.supports(Capability::ReducedMotionPreference));
        // Milestone 29: Direct2D canvases and raw-window-handle surfaces.
        assert!(capabilities.supports(Capability::CustomDrawing));
        assert!(capabilities.supports(Capability::NativeSurfaces));
        // Milestone 30: the file state store, single-instance deep links,
        // and session/power lifecycle notifications.
        assert!(capabilities.supports(Capability::StatePersistence));
        assert!(capabilities.supports(Capability::DeepLinks));
        assert!(capabilities.supports(Capability::Lifecycle));
        // Milestone 39.
        for realized in [
            Capability::Cursors,
            Capability::Hover,
            Capability::CommandShortcuts,
            Capability::RightToLeft,
            Capability::HostTraits,
            Capability::SystemAppearance,
            Capability::Permissions,
        ] {
            assert!(capabilities.supports(realized), "{realized:?}");
        }
        // System sharing is still only a portable contract, and no surface
        // beyond the main window is realized until Milestone 57.
        assert!(!capabilities.supports(Capability::SystemShare));
        for surface in [
            framework_core::SurfaceKind::Widget,
            framework_core::SurfaceKind::TrayExtra,
            framework_core::SurfaceKind::JumpList,
            framework_core::SurfaceKind::TaskbarProgress,
        ] {
            assert!(!capabilities.supports(Capability::Surface(surface)), "{surface:?}");
        }
    }
}
