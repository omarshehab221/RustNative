//! The renderer proper: applies a `TreeDiff` to native objects, then runs
//! layout and positions them.
//!
//! [`Renderer`] is a **coordinator**, in the sense the standards audit's
//! P1.21 finding uses the word: it sequences the phases of a render and
//! owns the state that spans them (the current snapshot, the layout
//! rectangles), and it delegates every phase's actual work to the module
//! that owns that phase's invariants. Control creation lives in
//! [`super::controls`], GDI resources in [`super::styling`], semantics in
//! [`super::accessibility`], viewport transforms in [`super::scrolling`].
//!
//! # Phase order
//!
//! ```text
//! render(root)
//!   ├─ snapshot the declarative tree, resolving theme styles
//!   ├─ diff against the previous snapshot
//!   ├─ apply each operation   → controls / styling / accessibility
//!   └─ if the diff touched geometry, relayout
//!         ├─ measure the window's client area
//!         ├─ run the platform-independent layout engine
//!         ├─ reconcile scroll offsets against the new ranges
//!         └─ position every native object
//! ```
//!
//! Layout deliberately runs *after* the whole native tree is reconciled,
//! never interleaved with it: every object that will exist this frame
//! exists before any of them is positioned, so geometry does not depend on
//! the order operations happened to arrive in.

use std::collections::{HashMap, HashSet};

use framework_core::{
    ControlState, LayoutEngine, NodeId, NodeKind, Overflow, Rect, Size, Theme, TreeDiff, TreeNode,
    TreeOp, TreeSnapshot,
};
use windows_sys::Win32::Foundation::{HWND, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetParent, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetParent, SetWindowPos,
    WM_SETFONT,
};

use super::super::EnableWindow;
use super::super::measure::WindowsIntrinsicMeasurer;
use super::super::registry::NativeObjectRegistry;
use super::super::user_data::BackgroundColorSlot;
use super::super::win32::{best_effort, ignored_by_contract, informational};
use super::accessibility::AccessibilityBridge;
use super::controls;
use super::scrolling::{ScrollState, dimension_to_u32};
use super::styling::StyleCache;
use crate::Error;

/// Reconciles one window's native Win32 objects against the framework's
/// declarative tree.
#[derive(Debug)]
pub(crate) struct Renderer {
    /// Every native object this window owns, in both directions.
    pub(crate) registry: NativeObjectRegistry,
    /// Realized visual styles and live interaction state.
    pub(crate) styles: StyleCache,
    /// The tree currently realized natively, and the baseline the next
    /// render diffs against.
    pub(crate) snapshot: TreeSnapshot,
    /// Node identities whose next `EN_CHANGE` came from this renderer's own
    /// `SetWindowTextW` rather than from the person typing.
    pub(crate) suppress_text_change: HashSet<NodeId>,
    layout: HashMap<NodeId, Rect>,
    scroll: ScrollState,
    accessibility: AccessibilityBridge,
    layout_engine: LayoutEngine,
}

impl Renderer {
    pub(crate) fn new() -> Self {
        Self {
            registry: NativeObjectRegistry::default(),
            styles: StyleCache::default(),
            snapshot: TreeSnapshot::default(),
            suppress_text_change: HashSet::new(),
            layout: HashMap::new(),
            scroll: ScrollState::default(),
            accessibility: AccessibilityBridge::default(),
            layout_engine: LayoutEngine,
        }
    }

    /// Reconciles the native tree against `root`, then relayouts if the
    /// change could have affected geometry.
    pub(crate) fn render(
        &mut self,
        root: &framework_core::Node,
        window: HWND,
        theme: &Theme,
    ) -> Result<(), Error> {
        let next =
            TreeSnapshot::from_node_with_theme(root, theme).map_err(|error| match error {
                framework_core::TreeError::DuplicateNodeId(id) => {
                    Error::DuplicateNodeId(id.get().to_string())
                }
            })?;

        let diff = TreeDiff::between(&self.snapshot, &next);
        self.styles.set_theme(theme.clone());
        for operation in diff.operations() {
            self.apply_operation(operation, window)?;
        }

        let layout_invalidated = diff.invalidates_layout();
        self.snapshot = next;
        // Interaction state (hover, press, focus) for nodes that no longer
        // exist would otherwise accumulate for the life of the window.
        let snapshot = &self.snapshot;
        self.styles.retain_interaction(|id| snapshot.contains(id));
        if layout_invalidated {
            self.relayout(window);
        }
        Ok(())
    }

    /// Runs layout for the whole window and applies the result to every
    /// native object.
    pub(crate) fn relayout(&mut self, window: HWND) {
        let mut client = RECT::default();
        // SAFETY: `window` is a live HWND owned by this renderer's window
        // for the duration of `relayout`; `client` is a valid, exclusively
        // borrowed `RECT` for `GetClientRect` to write into.
        let read_client_rect = unsafe { GetClientRect(window, &raw mut client) } != 0;
        // Best effort: a zeroed `RECT` lays the tree out at zero size,
        // which is the correct result for a window with no client area
        // (minimized, or mid-destruction) and self-corrects on the next
        // `WM_SIZE`.
        best_effort(
            read_client_rect,
            "GetClientRect(window)",
            "layout falls back to a zero-size client",
        );

        let size = Size::new(
            dimension_to_u32(client.right - client.left),
            dimension_to_u32(client.bottom - client.top),
        );
        let measurer = WindowsIntrinsicMeasurer { window };
        let output =
            self.layout_engine.layout_result_with(&self.snapshot, size, &measurer, &HashMap::new());

        self.layout = output.rects;
        let snapshot = &self.snapshot;
        self.scroll
            .adopt_layout(output.scroll_ranges, output.content_sizes, |id| snapshot.contains(id));

        // Layout is a distinct phase: only after the native tree is fully
        // reconciled do we apply geometry to every object. This makes layout
        // independent of reconciliation order.
        for id in self.snapshot.nodes().map(|node| node.id).collect::<Vec<_>>() {
            self.position_node(id);
        }
    }

    /// Scrolls a container by a wheel/keyboard delta, without rerendering.
    pub(crate) fn scroll_container(&mut self, id: NodeId, delta_x: i32, delta_y: i32) {
        if self.overflow_of(id) != Some(Overflow::Scroll) {
            return;
        }
        if self.scroll.scroll_by(id, delta_x, delta_y) {
            let Some(rect) = self.layout.get(&id).copied() else {
                return;
            };
            self.scroll.apply(id, &self.registry, rect);
        }
    }

    /// Walks up from `hwnd` to the nearest node whose container is
    /// scrollable, which is the one a wheel event over `hwnd` should move.
    pub(crate) fn scrollable_ancestor(&self, hwnd: HWND) -> Option<NodeId> {
        let mut current = hwnd;
        while !current.is_null() {
            if let Some(id) = self.registry.id_for_hwnd(current) {
                if self.overflow_of(id) == Some(Overflow::Scroll) {
                    return Some(id);
                }
            }
            // SAFETY: `current` was just checked non-null and is a live
            // HWND (either the caller's starting handle or a value
            // `GetParent` itself previously returned); a null return
            // (checked at the top of the loop) is the documented
            // "no parent" signal.
            current = unsafe { GetParent(current) };
        }
        None
    }

    /// Applies a live interaction transition (hover, press, focus) without
    /// rebuilding the declarative tree.
    ///
    /// Native focus and hover are transient interaction state, so a repaint
    /// caused by one must never cause an application render.
    pub(crate) fn set_control_state(&mut self, id: NodeId, state: ControlState) {
        let Some(node) = self.snapshot.get(id).cloned() else {
            return;
        };
        if node.disabled {
            return;
        }
        if self.styles.set_interaction(id, state) {
            self.apply_control_style(&node);
        }
    }

    /// A container node's overflow behavior, or `None` for anything that is
    /// not a container.
    fn overflow_of(&self, id: NodeId) -> Option<Overflow> {
        let node = self.snapshot.get(id)?;
        match node.kind {
            NodeKind::Column => node.column_style.map(|style| style.overflow),
            NodeKind::Row => node.row_style.map(|style| style.overflow),
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => None,
        }
    }

    fn apply_operation(&mut self, operation: &TreeOp, window: HWND) -> Result<(), Error> {
        match operation {
            TreeOp::Insert(node) => self.insert_node(node, window),
            TreeOp::Update(node) => self.update_node(node, window),
            TreeOp::Move { id, parent, .. } => {
                self.reparent(*id, *parent, window);
                Ok(())
            }
            TreeOp::Remove(node) => {
                self.remove_node(node.id);
                Ok(())
            }
        }
    }

    fn insert_node(&mut self, node: &TreeNode, window: HWND) -> Result<(), Error> {
        let parent = self.native_parent(node, window);
        controls::create(&mut self.registry, node, parent)?;
        self.apply_semantics_and_style(node);
        Ok(())
    }

    fn update_node(&mut self, node: &TreeNode, window: HWND) -> Result<(), Error> {
        if controls::needs_replacement(&self.registry, node) {
            self.remove_node(node.id);
            return self.insert_node(node, window);
        }

        self.apply_semantics_and_style(node);
        if controls::update_text(&self.registry, node)? {
            self.suppress_text_change.insert(node.id);
        }
        Ok(())
    }

    /// Both post-realization passes every insert and update needs, in the
    /// order they must run: semantics first (which can change window
    /// styles), then appearance (which repaints).
    fn apply_semantics_and_style(&mut self, node: &TreeNode) {
        if let Some(object) = self.registry.get(node.id) {
            self.accessibility.apply(object.hwnd(), node);
        }
        self.apply_control_style(node);
    }

    fn remove_node(&mut self, id: NodeId) {
        self.styles.forget(id);
        self.suppress_text_change.remove(&id);
        self.layout.remove(&id);
        if let Some(object) = self.registry.remove(id) {
            self.accessibility.forget(object.hwnd());
            object.destroy();
        }
    }

    fn reparent(&mut self, id: NodeId, parent: Option<NodeId>, window: HWND) {
        let Some(object) = self.registry.get(id) else {
            return;
        };
        let parent_hwnd = self.parent_hwnd(parent, window);
        // SAFETY: both handles are live native windows owned by this
        // renderer's registry (or the top-level window it renders into).
        //
        // `SetParent` returns the *previous* parent handle, not a status.
        informational(unsafe { SetParent(object.hwnd(), parent_hwnd) });
    }

    /// Realizes `node`'s resolved `visual_style` (font, foreground, and
    /// background) on its native object, and its `disabled` flag as a
    /// native `EnableWindow` toggle. Called after both insertion and
    /// in-place update, since either can change a node's style.
    ///
    /// Labels, buttons, and text inputs are repainted through
    /// `WM_CTLCOLOR*`, handled by whichever ancestor window owns the style
    /// cache (see `message_loop::control_color`). Containers paint their own
    /// background directly through `WM_ERASEBKGND`; since that handler
    /// cannot reach this `Renderer`, the resolved background color is
    /// additionally cached on the container's own `GWLP_USERDATA`.
    fn apply_control_style(&mut self, node: &TreeNode) {
        let Some(object) = self.registry.get(node.id) else {
            return;
        };
        let hwnd = object.hwnd();
        let content_hwnd = object.content_hwnd();

        let style = self.styles.realize(node.id, node.kind, &node.style_override, node.disabled);
        let font = style.font;
        let background = style.background;

        if !font.is_null() {
            // SAFETY: `hwnd`/`content_hwnd` are live HWNDs owned by this
            // renderer's registry; `font` is a valid `HFONT` kept alive in
            // the style cache for at least as long as any control can still
            // reference it (until the next realization or node removal
            // replaces/frees it); `WM_SETFONT`'s `lParam` of `1` requests
            // an immediate redraw, a documented valid value.
            //
            // `WM_SETFONT` is documented to return no meaningful value.
            unsafe {
                informational(SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1));
                if let Some(content_hwnd) = content_hwnd {
                    informational(SendMessageW(content_hwnd, WM_SETFONT, font as WPARAM, 1));
                }
            }
        }

        match node.kind {
            // This container-only slot is reserved for the resolved
            // background `COLORREF` (read back by the `WM_ERASEBKGND`
            // handler in `container_proc`, through
            // `styling::background_brush_for`), never aliased with the
            // `Runtime` pointer stored there for top-level windows — see
            // `user_data`'s module docs.
            NodeKind::Column | NodeKind::Row => {
                BackgroundColorSlot::set(hwnd, background);
                if let Some(content_hwnd) = content_hwnd {
                    BackgroundColorSlot::set(content_hwnd, background);
                }
            }
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {}
        }

        // SAFETY: `hwnd` is a live HWND owned by this renderer's registry;
        // a null `lpRect` is `InvalidateRect`'s documented way to
        // invalidate the entire client area.
        unsafe {
            // `EnableWindow` returns the window's *previous* disabled state.
            ignored_by_contract(EnableWindow(hwnd, i32::from(!node.disabled)));
            let invalidated = InvalidateRect(hwnd, std::ptr::null(), 1) != 0;
            // Best effort: a missed invalidation means the control keeps
            // its current pixels until something else repaints it.
            best_effort(invalidated, "InvalidateRect", "the control repaints on the next paint");
        }
    }

    fn position_node(&self, id: NodeId) {
        let Some(object) = self.registry.get(id) else {
            return;
        };
        let Some(rect) = self.layout.get(&id).copied() else {
            return;
        };

        // The layout engine always returns rectangles relative to the native
        // parent. Scrolling is deliberately NOT part of those rectangles;
        // see `super::scrolling`.
        // SAFETY: `object.hwnd()` is a live HWND owned by this renderer's
        // registry; a null `hWndInsertAfter` with `SWP_NOZORDER` leaves
        // z-order untouched, per its documented contract.
        let moved = unsafe {
            SetWindowPos(
                object.hwnd(),
                std::ptr::null_mut(),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        } != 0;
        // Best effort: the control keeps its previous geometry until the
        // next layout pass, and there is no caller that could act on a
        // single control failing to move.
        best_effort(moved, "SetWindowPos(layout)", "the control keeps its previous geometry");

        if object.content_hwnd().is_some() {
            self.scroll.apply(id, &self.registry, rect);
        }
    }

    /// The native window a node's control should be created inside: its
    /// parent's *content* host if the parent is a scrollable container,
    /// otherwise the parent's own window, falling back to the top-level
    /// window for a root node.
    fn native_parent(&self, node: &TreeNode, window: HWND) -> HWND {
        self.parent_hwnd(node.parent, window)
    }

    fn parent_hwnd(&self, parent: Option<NodeId>, window: HWND) -> HWND {
        parent
            .and_then(|parent_id| self.registry.get(parent_id))
            .map_or(window, |object| object.content_hwnd().unwrap_or_else(|| object.hwnd()))
    }
}
