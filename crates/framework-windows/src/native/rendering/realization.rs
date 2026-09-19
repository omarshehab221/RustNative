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
    AnimatedProperty, AnimatedValue, ControlState, Frame, LayoutEngine, NodeId, NodeKind, Overflow,
    Point, Rect, Scalar, Size, StyleOverride, Theme, Transition, TreeDiff, TreeNode, TreeOp,
    TreeSnapshot, VisualStyle,
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
use super::animated::AnimatedOverrides;
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
    /// Per-frame animated values — see `rendering::animated`.
    animated: AnimatedOverrides,
    /// Transitions the last render or layout pass found, waiting for
    /// `native::animation` to start them.
    pending_transitions: Vec<TransitionRequest>,
    /// Nodes removed since the last time animation state was reconciled.
    removed_nodes: Vec<NodeId>,
    scroll: ScrollState,
    /// The semantic projection onto Win32 (tab stops, MSAA annotations) and
    /// UI Automation — see `rendering::accessibility` and `native::uia`.
    pub(crate) accessibility: AccessibilityBridge,
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
            animated: AnimatedOverrides::default(),
            pending_transitions: Vec::new(),
            removed_nodes: Vec::new(),
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
                    Error::DuplicateNodeId { node: id }
                }
            })?;

        let diff = TreeDiff::between(&self.snapshot, &next);
        self.styles.set_theme(theme.clone());
        for operation in diff.operations() {
            self.apply_operation(operation, window)?;
        }

        let layout_invalidated = diff.invalidates_layout();
        self.collect_appearance_transitions(&next);
        self.snapshot = next;
        if self.accessibility.commit(window, &self.snapshot, &self.registry) {
            crate::native::uia::schedule(window);
        }
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

        self.collect_geometry_transitions(&output.rects);
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

    /// The nearest scrollable container strictly between `hwnd` and the
    /// node `stop` (walking outward), if any — the one a wheel event over
    /// `hwnd` should scroll instead of reaching `stop`.
    pub(crate) fn scrollable_ancestor_below(&self, hwnd: HWND, stop: NodeId) -> Option<NodeId> {
        let mut current = hwnd;
        while !current.is_null() {
            if let Some(id) = self.registry.id_for_hwnd(current) {
                if id == stop {
                    return None;
                }
                if self.overflow_of(id) == Some(Overflow::Scroll) {
                    return Some(id);
                }
            }
            // SAFETY: `current` was just checked non-null and is a live
            // HWND (the caller's or one `GetParent` returned); a null return
            // ends the walk.
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
        if let Some(object) = self.registry.get(node.id) {
            AccessibilityBridge::attach(object.hwnd(), node.kind);
        }
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
        self.apply_opacity(node.id);
    }

    fn remove_node(&mut self, id: NodeId) {
        self.animated.forget(id);
        self.removed_nodes.push(id);
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

        let style_override = self.animated_override(node);
        let style = self.styles.realize(node.id, node.kind, &style_override, node.disabled);
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
        // Whatever is animating this node wins over the laid-out geometry
        // until its animation ends (see `rendering::animated`).
        let rect = self.animated.rect_for(id, rect);

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

    /// A transition this renderer found: a property of a node that a
    /// render changed, and which the node asked to animate.
    ///
    /// Detecting it here is deliberate — this is the only place that knows
    /// both what was on screen and what the new tree says — while starting
    /// it belongs to `native::animation`, which owns the timeline.
    pub(crate) fn take_transitions(&mut self) -> Vec<TransitionRequest> {
        std::mem::take(&mut self.pending_transitions)
    }

    /// Nodes removed since this was last called, whose animations are over
    /// because the node is.
    pub(crate) fn forgotten_nodes(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.removed_nodes)
    }

    /// A property's value right now: what is animating, or what the
    /// rendered tree says. This is where an explicit animation starts from
    /// when it does not say.
    pub(crate) fn current_value(
        &self,
        node: NodeId,
        property: AnimatedProperty,
    ) -> Option<AnimatedValue> {
        let overrides = self.animated.get(node);
        let rendered = self.snapshot.get(node)?;
        Some(match property {
            AnimatedProperty::Position => {
                let rect = self.layout.get(&node).copied()?;
                AnimatedValue::Offset(overrides.position.unwrap_or(Point::new(rect.x, rect.y)))
            }
            AnimatedProperty::Size => {
                let rect = self.layout.get(&node).copied()?;
                AnimatedValue::Size(overrides.size.unwrap_or(size_of(rect)))
            }
            AnimatedProperty::Translation => {
                AnimatedValue::Offset(overrides.translation.unwrap_or(Point::new(0, 0)))
            }
            AnimatedProperty::Opacity => {
                AnimatedValue::Scalar(overrides.opacity.unwrap_or(rendered.opacity))
            }
            AnimatedProperty::Background => AnimatedValue::Color(
                overrides
                    .background
                    .or(rendered.visual_style.properties().background_override())?,
            ),
            AnimatedProperty::Foreground => AnimatedValue::Color(
                overrides
                    .foreground
                    .or(rendered.visual_style.properties().foreground_override())?,
            ),
            // A property this backend does not realize yet: nothing to
            // animate from, so the animation starts at its declared value.
            _ => return None,
        })
    }

    /// Applies one frame to its native object, if anything actually
    /// changed.
    pub(crate) fn apply_animation_frame(&mut self, frame: &Frame) {
        if !self.animated.set(frame.node, frame.property, frame.value) {
            return;
        }
        match frame.property {
            AnimatedProperty::Position | AnimatedProperty::Size | AnimatedProperty::Translation => {
                self.position_node(frame.node);
            }
            AnimatedProperty::Opacity => self.apply_opacity(frame.node),
            AnimatedProperty::Background | AnimatedProperty::Foreground => {
                if let Some(node) = self.snapshot.get(frame.node).cloned() {
                    self.apply_control_style(&node);
                }
            }
            // As in `current_value`: a property this backend does not
            // realize is not applied rather than applied wrongly.
            _ => {}
        }
    }

    /// Realizes a node's opacity — its own, or whatever is animating it —
    /// as a layered window.
    pub(crate) fn apply_opacity(&self, id: NodeId) {
        let Some(object) = self.registry.get(id) else {
            return;
        };
        let declared = self.snapshot.get(id).map_or(1.0, |node| node.opacity.get());
        let opacity = self.animated.get(id).opacity.map_or(declared, Scalar::get);
        super::styling::set_opacity(object.hwnd(), opacity);
    }

    /// Finds the appearance transitions a render started: a node that
    /// declared one, whose property the new tree changed.
    ///
    /// Collected into a local list first because the comparison borrows the
    /// *old* snapshot while the result belongs to this renderer.
    fn collect_appearance_transitions(&mut self, next: &TreeSnapshot) {
        let mut found = Vec::new();
        for node in next.nodes() {
            if node.transitions.is_empty() {
                continue;
            }
            let Some(previous) = self.snapshot.get(node.id) else {
                // A node appearing does not transition: there is nothing on
                // screen to move from.
                continue;
            };
            let overrides = self.animated.get(node.id);
            for declared in &node.transitions {
                let (from, to) = match declared.property {
                    AnimatedProperty::Opacity => (
                        AnimatedValue::Scalar(overrides.opacity.unwrap_or(previous.opacity)),
                        AnimatedValue::Scalar(node.opacity),
                    ),
                    AnimatedProperty::Background => {
                        let before = overrides
                            .background
                            .or(previous.visual_style.properties().background_override());
                        let after = node.visual_style.properties().background_override();
                        match (before, after) {
                            (Some(before), Some(after)) => {
                                (AnimatedValue::Color(before), AnimatedValue::Color(after))
                            }
                            // Nothing resolved a color for this node, so
                            // there is nothing to interpolate between.
                            _ => continue,
                        }
                    }
                    AnimatedProperty::Foreground => {
                        let before = overrides
                            .foreground
                            .or(previous.visual_style.properties().foreground_override());
                        let after = node.visual_style.properties().foreground_override();
                        match (before, after) {
                            (Some(before), Some(after)) => {
                                (AnimatedValue::Color(before), AnimatedValue::Color(after))
                            }
                            _ => continue,
                        }
                    }
                    // Geometry transitions are found by the layout pass,
                    // which is the only place that knows the rectangles.
                    _ => continue,
                };
                if from != to {
                    found.push(TransitionRequest {
                        node: node.id,
                        property: declared.property,
                        from,
                        to,
                        transition: declared.transition,
                    });
                }
            }
        }
        for request in &found {
            self.pin_transition_start(request);
        }
        self.pending_transitions.extend(found);
    }

    /// Holds a transitioned property at the value it is animating *from*,
    /// so applying the new tree does not make it jump before the first
    /// frame arrives. The timeline takes over from here, and clears the
    /// override when it finishes.
    fn pin_transition_start(&mut self, request: &TransitionRequest) {
        self.animated.set(request.node, request.property, Some(request.from));
    }

    fn collect_geometry_transitions(&mut self, next: &HashMap<NodeId, Rect>) {
        let mut geometry = Vec::new();
        for node in self.snapshot.nodes() {
            if node.transitions.is_empty() {
                continue;
            }
            let (Some(previous), Some(new)) =
                (self.layout.get(&node.id).copied(), next.get(&node.id).copied())
            else {
                continue;
            };
            let overrides = self.animated.get(node.id);
            for declared in &node.transitions {
                let (from, to) = match declared.property {
                    AnimatedProperty::Position => (
                        AnimatedValue::Offset(
                            overrides.position.unwrap_or(Point::new(previous.x, previous.y)),
                        ),
                        AnimatedValue::Offset(Point::new(new.x, new.y)),
                    ),
                    AnimatedProperty::Size => (
                        AnimatedValue::Size(overrides.size.unwrap_or(size_of(previous))),
                        AnimatedValue::Size(size_of(new)),
                    ),
                    _ => continue,
                };
                geometry.push(TransitionRequest {
                    node: node.id,
                    property: declared.property,
                    from,
                    to,
                    transition: declared.transition,
                });
            }
        }
        for request in &geometry {
            self.pin_transition_start(request);
        }
        self.pending_transitions.extend(geometry);
    }

    /// The node's style override with any animated colors applied, which is
    /// what a frame of a color transition changes.
    fn animated_override(&self, node: &TreeNode) -> StyleOverride {
        let overrides = self.animated.get(node.id);
        if overrides.background.is_none() && overrides.foreground.is_none() {
            return node.style_override.clone();
        }
        let mut style: VisualStyle = node.style_override.properties().clone();
        if let Some(background) = overrides.background {
            style = style.background(background);
        }
        if let Some(foreground) = overrides.foreground {
            style = style.foreground(foreground);
        }
        StyleOverride::new(style)
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

/// A transition the renderer found and `native::animation` will start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransitionRequest {
    /// The node to animate.
    pub(crate) node: NodeId,
    /// Which property moved.
    pub(crate) property: AnimatedProperty,
    /// What it is showing now.
    pub(crate) from: AnimatedValue,
    /// What the new tree says it should be.
    pub(crate) to: AnimatedValue,
    /// How the node asked it to move.
    pub(crate) transition: Transition,
}

/// A rectangle's size as the animatable value.
fn size_of(rect: Rect) -> Size {
    Size::new(dimension_to_u32(rect.width), dimension_to_u32(rect.height))
}
