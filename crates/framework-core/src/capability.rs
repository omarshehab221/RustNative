//! Portable platform-capability discovery.

use std::collections::BTreeSet;

/// A portable capability exposed by a platform adapter at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Capability {
    /// Reading/writing plain-text system clipboard content.
    Clipboard,
    /// Posting system notifications.
    Notifications,
    /// Capturing photos/video from a camera.
    Camera,
    /// Bluetooth device access.
    Bluetooth,
    /// Persistent key-value storage.
    Storage,
    /// Geolocation.
    Location,
    /// Native open/save/pick-folder file dialogs.
    FileDialogs,
    /// The platform's native share sheet/dialog.
    SystemShare,
    /// Launching URLs with the system's default handler.
    UrlLaunch,
    /// Opening more than one top-level window.
    MultipleWindows,
    /// Programmatic window placement/state management.
    WindowManagement,
    /// Reading the system's light/dark appearance/accent-color settings.
    SystemAppearance,
    /// Drag-and-drop input.
    DragAndDrop,
    /// Native menu bars/context menus.
    Menus,
}

/// The set of [`Capability`]s one platform adapter actually realizes.
///
/// A backend should only ever advertise a capability once it genuinely
/// realizes the corresponding behavior — never speculatively, and never for
/// a portable contract that exists in `framework-core` but has no native
/// implementation yet. See `framework-windows`'s own
/// `capabilities_only_advertise_realized_backend_features` test for how a
/// backend is expected to enforce this about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformCapabilities {
    available: BTreeSet<Capability>,
}

impl PlatformCapabilities {
    /// Creates a capability set from the capabilities a backend realizes.
    pub fn new(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self { available: capabilities.into_iter().collect() }
    }

    /// Returns whether `capability` is realized.
    #[must_use]
    pub fn supports(&self, capability: Capability) -> bool {
        self.available.contains(&capability)
    }

    /// Iterates every realized capability, in a stable order.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.available.iter().copied()
    }
}
