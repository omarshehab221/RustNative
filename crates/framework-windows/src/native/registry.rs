//! Tracks every native Win32 object created for a `NodeId`, in both
//! directions (`NodeId -> NativeObject` and `HWND -> NodeId`).

use std::collections::HashMap;

use framework_core::NodeId;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;

use crate::Error;

#[derive(Debug, Default)]
pub(crate) struct NativeObjectRegistry {
    objects: HashMap<NodeId, NativeObject>,
    by_hwnd: HashMap<HWND, NodeId>,
    /// Each object's top-level window, recorded at insertion — by the time
    /// an object is removed its window may already be gone, so it cannot
    /// be asked then.
    top_levels: HashMap<NodeId, isize>,
}

#[derive(Debug)]
pub(crate) enum NativeObject {
    Container {
        viewport: HWND,
        content: HWND,
    },
    Label(HWND),
    Button(HWND),
    TextInput(HWND),
    /// A Direct2D canvas window (see `native::graphics::canvas`).
    Canvas(HWND),
    /// A system tab control (see `rendering::tabs`).
    TabBar(HWND),
    /// A native surface window, and the id the application knows it by.
    Surface {
        hwnd: HWND,
        id: framework_core::SurfaceId,
    },
    /// A foreign object a registered factory made (see `native::foreign`).
    Foreign {
        hwnd: HWND,
        kind: String,
        ownership: super::foreign::Ownership,
    },
}

impl NativeObject {
    pub(crate) fn hwnd(&self) -> HWND {
        match self {
            Self::Container { viewport, .. } => *viewport,
            Self::Label(hwnd)
            | Self::Button(hwnd)
            | Self::TextInput(hwnd)
            | Self::Canvas(hwnd)
            | Self::TabBar(hwnd)
            | Self::Surface { hwnd, .. }
            | Self::Foreign { hwnd, .. } => *hwnd,
        }
    }

    pub(crate) fn content_hwnd(&self) -> Option<HWND> {
        match self {
            Self::Container { content, .. } => Some(*content),
            _ => None,
        }
    }

    pub(crate) fn destroy(self) {
        if let Self::Foreign { hwnd, ownership, .. } = self {
            super::foreign::release(hwnd, ownership);
            return;
        }
        // SAFETY: the registry exclusively owns every HWND stored here.
        unsafe {
            DestroyWindow(self.hwnd());
        }
    }
}

/// The top-level window `hwnd` belongs to, as an integer.
fn top_level(hwnd: HWND) -> isize {
    // The framework root, which for an embedded tree is not `GA_ROOT`.
    let root = super::context::root_window(hwnd);
    if root.is_null() { hwnd as isize } else { root as isize }
}

impl NativeObjectRegistry {
    /// Hands back every borrowed foreign object — called as the window
    /// closes, before Windows destroys its children with it.
    pub(crate) fn release_borrowed_foreign(&self) {
        for object in self.objects.values() {
            if let NativeObject::Foreign {
                hwnd,
                ownership: super::foreign::Ownership::Borrowed,
                ..
            } = object
            {
                super::foreign::release(*hwnd, super::foreign::Ownership::Borrowed);
            }
        }
    }

    pub(crate) fn get(&self, id: NodeId) -> Option<&NativeObject> {
        self.objects.get(&id)
    }

    pub(crate) fn insert(&mut self, id: NodeId, object: NativeObject) -> Result<(), Error> {
        if self.objects.contains_key(&id) {
            return Err(Error::DuplicateNodeId { node: id });
        }

        self.by_hwnd.insert(object.hwnd(), id);
        if let Some(content) = object.content_hwnd() {
            self.by_hwnd.insert(content, id);
        }
        let root = top_level(object.hwnd());
        self.top_levels.insert(id, root);
        crate::handle::record(id, root, object.hwnd() as isize);
        self.objects.insert(id, object);
        Ok(())
    }

    pub(crate) fn remove(&mut self, id: NodeId) -> Option<NativeObject> {
        let object = self.objects.remove(&id)?;
        if let Some(root) = self.top_levels.remove(&id) {
            crate::handle::forget(id, root);
        }
        self.by_hwnd.remove(&object.hwnd());
        if let Some(content) = object.content_hwnd() {
            self.by_hwnd.remove(&content);
        }
        Some(object)
    }

    pub(crate) fn id_for_hwnd(&self, hwnd: HWND) -> Option<NodeId> {
        self.by_hwnd.get(&hwnd).copied()
    }
}

impl Drop for NativeObjectRegistry {
    fn drop(&mut self) {
        self.by_hwnd.clear();
        for (id, root) in self.top_levels.drain() {
            crate::handle::forget(id, root);
        }
        for (_, object) in self.objects.drain() {
            object.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

    use super::*;
    use crate::native::test_support::TestWindow;

    fn node_id(local: u64) -> NodeId {
        NodeId::from_key(&local.to_string())
    }

    #[test]
    fn insert_then_get_round_trips_by_node_id() {
        let window = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(1);
        registry.insert(id, NativeObject::Label(window.hwnd)).expect("first insert must succeed");
        assert_eq!(registry.get(id).map(NativeObject::hwnd), Some(window.hwnd));
    }

    #[test]
    fn insert_rejects_a_duplicate_node_id() {
        let window_a = TestWindow::new();
        let window_b = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(1);
        registry.insert(id, NativeObject::Label(window_a.hwnd)).expect("first insert succeeds");
        let result = registry.insert(id, NativeObject::Label(window_b.hwnd));
        assert!(matches!(result, Err(Error::DuplicateNodeId { .. })));
        // The rejected second insert must not have clobbered the first.
        assert_eq!(registry.get(id).map(NativeObject::hwnd), Some(window_a.hwnd));
    }

    #[test]
    fn id_for_hwnd_is_a_real_reverse_lookup() {
        let window = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(7);
        registry.insert(id, NativeObject::Button(window.hwnd)).expect("insert succeeds");
        assert_eq!(registry.id_for_hwnd(window.hwnd), Some(id));
    }

    #[test]
    fn container_reverse_lookup_resolves_both_viewport_and_content_hwnds() {
        let viewport = TestWindow::new();
        let content = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(3);
        registry
            .insert(id, NativeObject::Container { viewport: viewport.hwnd, content: content.hwnd })
            .expect("insert succeeds");
        assert_eq!(registry.id_for_hwnd(viewport.hwnd), Some(id));
        assert_eq!(
            registry.id_for_hwnd(content.hwnd),
            Some(id),
            "a container's inner content HWND must resolve to the same NodeId as its viewport"
        );
    }

    #[test]
    fn remove_clears_both_the_forward_and_reverse_lookup() {
        let window = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(4);
        registry.insert(id, NativeObject::Label(window.hwnd)).expect("insert succeeds");
        // `NativeObject` has no `Drop` impl of its own — destruction is
        // always the explicit, manual `.destroy()` call
        // `NativeObjectRegistry::drop` makes, never an automatic side
        // effect of a value going out of scope — so simply letting
        // `removed` go out of scope here is harmless and does not
        // double-destroy `window.hwnd`.
        let _removed = registry.remove(id).expect("remove must return the just-inserted object");

        assert!(registry.get(id).is_none());
        assert!(registry.id_for_hwnd(window.hwnd).is_none());
    }

    #[test]
    fn dropping_the_registry_destroys_every_native_window_it_still_owns() {
        // Deliberately does not use `TestWindow` here: the whole point of
        // this test is to prove `NativeObjectRegistry::drop` itself
        // destroys the HWND, so ownership must transfer to the registry,
        // not stay with a `TestWindow` guard that would destroy it again.
        let window = TestWindow::new();
        let hwnd = window.hwnd;
        std::mem::forget(window);

        {
            let mut registry = NativeObjectRegistry::default();
            registry.insert(node_id(9), NativeObject::Label(hwnd)).expect("insert succeeds");
            // SAFETY: `hwnd` is still alive here — the registry has not
            // been dropped yet — and `IsWindow` takes no pointer
            // arguments beyond the handle itself.
            let is_window_before_drop = unsafe { IsWindow(hwnd) };
            assert_ne!(is_window_before_drop, 0, "hwnd must be a live window before drop");
        }

        // SAFETY: querying a handle after it may have been destroyed is
        // exactly what `IsWindow` is documented to make safe to do.
        let is_window_after_drop = unsafe { IsWindow(hwnd) };
        assert_eq!(
            is_window_after_drop, 0,
            "NativeObjectRegistry::drop must have destroyed every HWND it owned"
        );
    }
}
