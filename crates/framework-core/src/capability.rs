//! Portable platform-capability discovery.

use std::collections::BTreeSet;

/// A portable capability exposed by a platform adapter at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Capability {
    Clipboard,
    Notifications,
    Camera,
    Bluetooth,
    Storage,
    Location,
    FileDialogs,
    SystemShare,
    UrlLaunch,
    MultipleWindows,
    WindowManagement,
    SystemAppearance,
    DragAndDrop,
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
    pub fn new(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self { available: capabilities.into_iter().collect() }
    }
    #[must_use]
    pub fn supports(&self, capability: Capability) -> bool {
        self.available.contains(&capability)
    }
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.available.iter().copied()
    }
}
