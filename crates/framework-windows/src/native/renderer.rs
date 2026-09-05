//! Reconciles a `TreeDiff` between two `TreeSnapshot`s into real native
//! Win32 controls, and lays out and repaints them afterward.

use std::collections::HashMap;
use std::ptr::{null, null_mut};

use framework_core::{
    LayoutEngine, NodeId, NodeKind, Overflow, Point, Rect, Size, Theme, TreeDiff, TreeNode, TreeOp,
    TreeSnapshot,
};
use windows_sys::Win32::Foundation::{HWND, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
use windows_sys::Win32::System::SystemServices::{SS_LEFT, SS_NOPREFIX};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_PUSHBUTTON, CreateWindowExW, DestroyWindow, ES_AUTOHSCROLL, ES_LEFT, GWL_STYLE,
    GetClientRect, GetParent, GetWindowLongPtrW, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW,
    SetParent, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, WM_SETFONT, WS_BORDER, WS_CHILD,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_CONTROLPARENT, WS_TABSTOP, WS_VISIBLE,
};

use super::measure::{ControlStyle, WindowsIntrinsicMeasurer};
use super::registry::{NativeObject, NativeObjectRegistry};
use super::user_data::BackgroundColorSlot;
use super::util::{module_instance, wide, window_text};
use super::{CONTAINER_CLASS_NAME, EnableWindow};

/// Converts a Win32 `RECT`-derived dimension (`i32`, and documented by
/// every caller here to already be non-negative in the cases that matter)
/// to the unsigned `Size`/scroll-range representation this framework's
/// layout types use. A negative input — which should not occur for a real
/// window's client-area dimensions — is treated defensively as zero rather
/// than panicking or wrapping.
#[allow(clippy::cast_sign_loss)]
fn dimension_to_u32(value: i32) -> u32 {
    value.max(0) as u32
}

/// The inverse of [`dimension_to_u32`]: saturates at `i32::MAX` rather than
/// wrapping if a `u32` dimension from this framework's layout types
/// somehow exceeded it, which no real window or scroll range legitimately
/// does.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
fn dimension_to_i32(value: u32) -> i32 {
    value.min(i32::MAX as u32) as i32
}
use crate::Error;

#[derive(Debug)]
pub(crate) struct Renderer {
    pub(crate) registry: NativeObjectRegistry,
    pub(crate) styles: HashMap<NodeId, ControlStyle>,
    pub(crate) snapshot: TreeSnapshot,
    layout: HashMap<NodeId, Rect>,
    scroll_ranges: HashMap<NodeId, Size>,
    content_sizes: HashMap<NodeId, Size>,
    scroll_offsets: HashMap<NodeId, Point>,
    control_states: HashMap<NodeId, framework_core::ControlState>,
    theme: Theme,
    layout_engine: LayoutEngine,
    pub(crate) suppress_text_change: std::collections::HashSet<NodeId>,
}

impl Renderer {
    pub(crate) fn new() -> Self {
        Self {
            registry: NativeObjectRegistry::default(),
            styles: HashMap::new(),
            snapshot: TreeSnapshot::default(),
            layout: HashMap::new(),
            scroll_ranges: HashMap::new(),
            content_sizes: HashMap::new(),
            scroll_offsets: HashMap::new(),
            control_states: HashMap::new(),
            theme: Theme::default(),
            layout_engine: LayoutEngine,
            suppress_text_change: std::collections::HashSet::new(),
        }
    }

    pub(crate) fn scroll_container(&mut self, id: NodeId, delta_x: i32, delta_y: i32) {
        let Some(node) = self.snapshot.get(id) else {
            return;
        };
        let overflow = match node.kind {
            NodeKind::Column => node.column_style.map(|style| style.overflow),
            NodeKind::Row => node.row_style.map(|style| style.overflow),
            _ => None,
        };
        if overflow != Some(Overflow::Scroll) {
            return;
        }

        let range = self.scroll_ranges.get(&id).copied().unwrap_or(Size::new(0, 0));
        let current = self.scroll_offsets.get(&id).copied().unwrap_or_default();
        let next = Point::new(
            (current.x + delta_x).clamp(0, dimension_to_i32(range.width)),
            (current.y + delta_y).clamp(0, dimension_to_i32(range.height)),
        );
        if next == current {
            return;
        }

        self.scroll_offsets.insert(id, next);
        self.apply_scroll_offset(id);
    }

    fn apply_scroll_offset(&self, id: NodeId) {
        let Some(NativeObject::Container { content, .. }) = self.registry.get(id) else {
            return;
        };

        let scroll = self.scroll_offsets.get(&id).copied().unwrap_or_default();
        let Some(rect) = self.layout.get(&id) else {
            return;
        };
        let content_size = self
            .content_sizes
            .get(&id)
            .copied()
            .unwrap_or(Size::new(dimension_to_u32(rect.width), dimension_to_u32(rect.height)));

        let width = dimension_to_i32(content_size.width.max(dimension_to_u32(rect.width)));
        let height = dimension_to_i32(content_size.height.max(dimension_to_u32(rect.height)));

        // Scrolling is a transient viewport transform. Do not rerun the
        // application component, tree reconciliation, or layout engine.
        // The content host alone moves inside the stable viewport.
        // SAFETY: `content` is a live HWND owned by this node's registry
        // entry (matched above); a null `hWndInsertAfter` combined with
        // `SWP_NOZORDER` is the documented way to leave z-order
        // untouched, so no window-handle argument beyond `*content`
        // itself is dereferenced by this call.
        unsafe {
            SetWindowPos(
                *content,
                null_mut(),
                -scroll.x,
                -scroll.y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }

    pub(crate) fn scrollable_ancestor(&self, hwnd: HWND) -> Option<NodeId> {
        let mut current = hwnd;
        while !current.is_null() {
            if let Some(id) = self.registry.id_for_hwnd(current) {
                if let Some(node) = self.snapshot.get(id) {
                    let overflow = match node.kind {
                        NodeKind::Column => node.column_style.map(|style| style.overflow),
                        NodeKind::Row => node.row_style.map(|style| style.overflow),
                        _ => None,
                    };
                    if overflow == Some(Overflow::Scroll) {
                        return Some(id);
                    }
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
        self.theme = theme.clone();
        for operation in diff.operations() {
            self.apply_operation(operation, window)?;
        }

        let layout_invalidated = diff.invalidates_layout();
        self.snapshot = next;
        self.control_states.retain(|id, _| self.snapshot.contains(*id));
        if layout_invalidated {
            self.relayout(window);
        }
        Ok(())
    }

    pub(crate) fn relayout(&mut self, window: HWND) {
        let mut client = RECT::default();
        // SAFETY: `window` is a live HWND owned by this renderer's
        // window for the duration of `relayout`; `client` is a valid,
        // exclusively borrowed `RECT` for `GetClientRect` to write into.
        unsafe {
            GetClientRect(window, &raw mut client);
        }

        let size = Size::new(
            dimension_to_u32(client.right - client.left),
            dimension_to_u32(client.bottom - client.top),
        );
        let measurer = WindowsIntrinsicMeasurer { window };
        let output =
            self.layout_engine.layout_result_with(&self.snapshot, size, &measurer, &HashMap::new());

        self.layout = output.rects;
        self.content_sizes = output.content_sizes;
        self.scroll_ranges = output.scroll_ranges;

        // Scroll offsets are runtime viewport state, not layout input.
        // Reconcile the set of offsets after layout changes, clamp them to
        // the new ranges, and apply the final transform without rebuilding
        // the component tree.
        self.scroll_offsets.retain(|id, _| self.snapshot.contains(*id));
        for (id, offset) in &mut self.scroll_offsets {
            if let Some(range) = self.scroll_ranges.get(id) {
                offset.x = offset.x.clamp(0, dimension_to_i32(range.width));
                offset.y = offset.y.clamp(0, dimension_to_i32(range.height));
            } else {
                offset.x = 0;
                offset.y = 0;
            }
        }

        // Layout is a distinct phase: only after the native tree is fully
        // reconciled do we apply geometry to every object. This makes layout
        // independent of reconciliation order.
        for node in self.snapshot.nodes() {
            self.position_node(node.id);
        }
    }

    fn apply_operation(&mut self, operation: &TreeOp, window: HWND) -> Result<(), Error> {
        match operation {
            TreeOp::Insert(node) => self.insert_node(node, window),
            TreeOp::Update(node) => self.update_node(node, window),
            TreeOp::Move { id, parent, .. } => {
                if let Some(object) = self.registry.get(*id) {
                    let parent_hwnd = parent
                        .and_then(|parent_id| self.registry.get(parent_id))
                        .and_then(NativeObject::content_hwnd)
                        .or_else(|| {
                            parent.and_then(|parent_id| {
                                self.registry.get(parent_id).map(NativeObject::hwnd)
                            })
                        })
                        .unwrap_or(window);

                    // SAFETY: both HWNDs are valid native windows owned by the renderer.
                    unsafe {
                        SetParent(object.hwnd(), parent_hwnd);
                    }
                }

                Ok(())
            }
            TreeOp::Remove(node) => {
                self.styles.remove(&node.id);
                if let Some(object) = self.registry.remove(node.id) {
                    object.destroy();
                }
                Ok(())
            }
        }
    }

    fn sync_accessibility_state(&self, node: &TreeNode) {
        let Some(object) = self.registry.get(node.id) else {
            return;
        };
        // Standard controls provide their native role/text semantics. Keep
        // portable focusability in sync in both directions so a rerender
        // cannot leave a control in the keyboard tab order by accident.
        let hwnd = object.hwnd();
        // SAFETY: `hwnd` is a live HWND owned by this node's registry
        // entry (matched above); `GWL_STYLE` is a documented, always-
        // valid index for any window.
        let current = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
        // `WS_TABSTOP` is a small, fixed Win32 style-bit constant; it can
        // never approach `isize::MAX`/wrap the sign bit on either a
        // 32-bit or 64-bit `isize`.
        #[allow(clippy::cast_possible_wrap)]
        let tabstop_bit = WS_TABSTOP as isize;
        let next = if node.accessibility.is_focusable() && !node.disabled {
            current | tabstop_bit
        } else {
            current & !tabstop_bit
        };
        if next != current {
            // SAFETY: `hwnd` is the same live HWND validated above;
            // `GWL_STYLE` is a documented, always-valid index; `next`
            // was derived from `current` (itself just read back from
            // this same slot) with only the single documented
            // `WS_TABSTOP` bit toggled.
            unsafe {
                SetWindowLongPtrW(hwnd, GWL_STYLE, next);
            }
        }
    }

    /// Realizes `node`'s resolved `visual_style` (font, foreground, and
    /// background) on its native object, and its `disabled` flag as a
    /// native `EnableWindow` toggle. Called after both insertion and
    /// in-place update, since either can change a node's style.
    ///
    /// Labels, buttons, and text inputs are repainted through
    /// `WM_CTLCOLOR*`, handled by whichever ancestor window owns
    /// `styles` (see `window_proc`). Containers paint their own
    /// background directly through `WM_ERASEBKGND`; since that handler
    /// cannot reach this `Renderer`, the resolved background color is
    /// additionally cached on the container's own `GWLP_USERDATA`.
    fn apply_control_style(&mut self, node: &TreeNode) {
        let Some(object) = self.registry.get(node.id) else {
            return;
        };
        let hwnd = object.hwnd();
        let content_hwnd = object.content_hwnd();

        let state = if node.disabled {
            framework_core::ControlState::Disabled
        } else {
            self.control_states
                .get(&node.id)
                .copied()
                .unwrap_or(framework_core::ControlState::Normal)
        };
        let style =
            ControlStyle::resolve(&self.theme.resolve(node.kind, state, &node.style_override));

        if !style.font.is_null() {
            // SAFETY: `hwnd`/`content_hwnd` are live HWNDs owned by this
            // renderer's registry; `style.font` was just checked
            // non-null and is a valid `HFONT` kept alive in `self.styles`
            // for at least as long as any control can still reference it
            // (until the next `apply_control_style` or node removal
            // replaces/frees it); `WM_SETFONT`'s `lParam` of `1`
            // requests an immediate redraw, a documented valid value.
            unsafe {
                SendMessageW(hwnd, WM_SETFONT, style.font as WPARAM, 1);
                if let Some(content_hwnd) = content_hwnd {
                    SendMessageW(content_hwnd, WM_SETFONT, style.font as WPARAM, 1);
                }
            }
        }

        match node.kind {
            // This container-only slot is reserved for the resolved
            // background `COLORREF` (read back by the `WM_ERASEBKGND`
            // handler in `container_proc`), never aliased with the
            // `Runtime` pointer stored there for the top-level and other
            // windows — see `user_data`'s module docs.
            NodeKind::Column | NodeKind::Row => {
                BackgroundColorSlot::set(hwnd, style.background);
                if let Some(content_hwnd) = content_hwnd {
                    BackgroundColorSlot::set(content_hwnd, style.background);
                }
            }
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {}
        }

        // SAFETY: `hwnd` is a live HWND owned by this renderer's
        // registry; a null `lpRect` is `InvalidateRect`'s documented way
        // to invalidate the entire client area.
        unsafe {
            EnableWindow(hwnd, i32::from(!node.disabled));
            InvalidateRect(hwnd, null(), 1);
        }

        self.styles.insert(node.id, style);
    }

    /// Applies a live state transition without rebuilding the declarative
    /// component tree. Native focus is transient interaction state, so a
    /// focus repaint must not cause an application render.
    pub(crate) fn set_control_state(&mut self, id: NodeId, state: framework_core::ControlState) {
        let Some(node) = self.snapshot.get(id).cloned() else {
            return;
        };
        if node.disabled {
            return;
        }
        if state == framework_core::ControlState::Normal {
            self.control_states.remove(&id);
        } else {
            self.control_states.insert(id, state);
        }
        self.apply_control_style(&node);
    }

    fn insert_node(&mut self, node: &TreeNode, window: HWND) -> Result<(), Error> {
        let parent = native_parent(node, &self.registry, window);

        match node.kind {
            NodeKind::Column | NodeKind::Row => self.create_container(node, parent)?,
            NodeKind::Label => self.create_label(node, parent)?,
            NodeKind::Button => self.create_button(node, parent)?,
            NodeKind::TextInput => self.create_text_input(node, parent)?,
        }

        self.sync_accessibility_state(node);
        self.apply_control_style(node);
        Ok(())
    }

    fn update_node(&mut self, node: &TreeNode, window: HWND) -> Result<(), Error> {
        let needs_replacement = match (node.kind, self.registry.get(node.id)) {
            (NodeKind::Column | NodeKind::Row, Some(NativeObject::Container { .. }))
            | (NodeKind::Label, Some(NativeObject::Label(_)))
            | (NodeKind::Button, Some(NativeObject::Button(_)))
            | (NodeKind::TextInput, Some(NativeObject::TextInput(_))) => false,
            // Any other combination is either no existing native object
            // yet, or one of the wrong kind (e.g. a node's `NodeKind`
            // changed identity-preservingly across a rerender) — either
            // way the existing native object (if any) must be torn down
            // and rebuilt from scratch.
            _ => true,
        };

        if needs_replacement {
            if let Some(object) = self.registry.remove(node.id) {
                object.destroy();
            }
            return self.insert_node(node, window);
        }

        self.sync_accessibility_state(node);
        self.apply_control_style(node);

        match node.kind {
            NodeKind::Column | NodeKind::Row => {}
            NodeKind::Label | NodeKind::Button => {
                if let Some(object) = self.registry.get(node.id) {
                    let text = wide(node.text.as_deref().unwrap_or_default());
                    // SAFETY: `object.hwnd()` is a live HWND owned by
                    // this renderer's registry; `text` is a
                    // NUL-terminated wide buffer kept alive for the
                    // duration of this synchronous call.
                    unsafe {
                        SetWindowTextW(object.hwnd(), text.as_ptr());
                    }
                }
            }
            NodeKind::TextInput => {
                if let Some(value) = node.text.as_deref() {
                    let Some(hwnd) = self.registry.get(node.id).map(NativeObject::hwnd) else {
                        return Ok(());
                    };
                    let current = window_text(hwnd);
                    if current != value {
                        self.suppress_text_change.insert(node.id);
                        let text = wide(value);
                        // SAFETY: `hwnd` is a live HWND owned by this
                        // renderer's registry; `text` is a
                        // NUL-terminated wide buffer kept alive for the
                        // duration of this synchronous call.
                        let succeeded = unsafe { SetWindowTextW(hwnd, text.as_ptr()) } != 0;
                        if !succeeded {
                            self.suppress_text_change.remove(&node.id);
                            return Err(Error::windows_api("SetWindowTextW(EDIT)"));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn position_node(&self, id: NodeId) {
        let Some(object) = self.registry.get(id) else {
            return;
        };
        let Some(rect) = self.layout.get(&id) else {
            return;
        };

        // The layout engine always returns rectangles relative to the native
        // parent. Scrolling is deliberately NOT part of those rectangles.
        // Instead, a scrollable container owns a viewport and a content host;
        // only the content host is translated by the scroll offset.
        // SAFETY: `object.hwnd()` is a live HWND owned by this
        // renderer's registry; a null `hWndInsertAfter` with
        // `SWP_NOZORDER` leaves z-order untouched, per its documented
        // contract.
        unsafe {
            SetWindowPos(
                object.hwnd(),
                null_mut(),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }

        if let Some(content) = object.content_hwnd() {
            let scroll = self.scroll_offsets.get(&id).copied().unwrap_or_default();
            let content_size = self
                .content_sizes
                .get(&id)
                .copied()
                .unwrap_or(Size::new(dimension_to_u32(rect.width), dimension_to_u32(rect.height)));
            let width = dimension_to_i32(content_size.width.max(dimension_to_u32(rect.width)));
            let height = dimension_to_i32(content_size.height.max(dimension_to_u32(rect.height)));

            // SAFETY: `content` is a live HWND owned by this node's
            // registry entry; a null `hWndInsertAfter` with
            // `SWP_NOZORDER` leaves z-order untouched, per its
            // documented contract.
            unsafe {
                SetWindowPos(
                    content,
                    null_mut(),
                    -scroll.x,
                    -scroll.y,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }
    }

    fn create_container(&mut self, node: &TreeNode, parent: HWND) -> Result<(), Error> {
        let class = wide(CONTAINER_CLASS_NAME);
        // SAFETY: `class` is a NUL-terminated wide buffer naming the
        // window class `register_window_classes` registers before any
        // window is created; `parent` is a live HWND owned by the
        // renderer (or the top-level `window` passed down from
        // `render`); a null `lpParam` is a documented valid value the
        // resulting `WM_NCCREATE`/`WM_CREATE` handlers here do not read;
        // a null return (checked below) is `CreateWindowExW`'s
        // documented failure signal.
        let viewport = unsafe {
            CreateWindowExW(
                WS_EX_CONTROLPARENT,
                class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                module_instance(),
                null(),
            )
        };

        if viewport.is_null() {
            return Err(Error::windows_api("CreateWindowExW(CONTAINER_VIEWPORT)"));
        }

        // SAFETY: same reasoning as the `viewport` creation above;
        // `viewport` was just checked non-null and is the live parent
        // HWND for this child window.
        let content = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                0,
                0,
                viewport,
                null_mut(),
                module_instance(),
                null(),
            )
        };

        if content.is_null() {
            // SAFETY: `viewport` was just created above and has not yet
            // been handed to the registry, so this function is still its
            // sole owner and must tear it down on this failure path.
            unsafe {
                DestroyWindow(viewport);
            }
            return Err(Error::windows_api("CreateWindowExW(CONTAINER_CONTENT)"));
        }

        if let Err(error) =
            self.registry.insert(node.id, NativeObject::Container { viewport, content })
        {
            // SAFETY: `content` and `viewport` were just created above
            // and have not yet been handed to the registry (the
            // `insert` above failed), so this function is still their
            // sole owner and must tear both down on this failure path.
            unsafe {
                DestroyWindow(content);
                DestroyWindow(viewport);
            }
            return Err(error);
        }

        Ok(())
    }

    fn create_label(&mut self, node: &TreeNode, parent: HWND) -> Result<(), Error> {
        let text = wide(node.text.as_deref().unwrap_or_default());
        let class = wide("STATIC");
        // SAFETY: `class`/`text` are NUL-terminated wide buffers naming
        // a predefined system window class and the initial window text;
        // `parent` is a live HWND owned by the renderer (or the
        // top-level `window` passed down from `render`); a null
        // `lpParam` is a documented valid value this class's default
        // window procedure does not read; a null return (checked below)
        // is `CreateWindowExW`'s documented failure signal.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | SS_LEFT | SS_NOPREFIX,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                module_instance(),
                null(),
            )
        };

        if hwnd.is_null() {
            return Err(Error::windows_api("CreateWindowExW(STATIC)"));
        }

        if let Err(error) = self.registry.insert(node.id, NativeObject::Label(hwnd)) {
            // SAFETY: `hwnd` was just created above and has not yet been
            // handed to the registry (the `insert` above failed), so
            // this function is still its sole owner and must tear it
            // down on this failure path.
            unsafe { DestroyWindow(hwnd) };
            return Err(error);
        }

        Ok(())
    }

    fn create_button(&mut self, node: &TreeNode, parent: HWND) -> Result<(), Error> {
        let text = wide(node.text.as_deref().unwrap_or_default());
        let class = wide("BUTTON");
        // SAFETY: same reasoning as `create_label` above, for the
        // predefined "BUTTON" system window class.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                module_instance(),
                null(),
            )
        };

        if hwnd.is_null() {
            return Err(Error::windows_api("CreateWindowExW(BUTTON)"));
        }

        if let Err(error) = self.registry.insert(node.id, NativeObject::Button(hwnd)) {
            // SAFETY: `hwnd` was just created above and has not yet been
            // handed to the registry (the `insert` above failed), so
            // this function is still its sole owner and must tear it
            // down on this failure path.
            unsafe { DestroyWindow(hwnd) };
            return Err(error);
        }

        Ok(())
    }
    fn create_text_input(&mut self, node: &TreeNode, parent: HWND) -> Result<(), Error> {
        let value = wide(node.text.as_deref().unwrap_or_default());
        let class = wide("EDIT");
        // SAFETY: same reasoning as `create_label` above, for the
        // predefined "EDIT" system window class.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                value.as_ptr(),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_BORDER
                    | ES_LEFT as u32
                    | ES_AUTOHSCROLL as u32,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                module_instance(),
                null(),
            )
        };

        if hwnd.is_null() {
            return Err(Error::windows_api("CreateWindowExW(EDIT)"));
        }

        if let Err(error) = self.registry.insert(node.id, NativeObject::TextInput(hwnd)) {
            // SAFETY: `hwnd` was just created above and has not yet been
            // handed to the registry (the `insert` above failed), so
            // this function is still its sole owner and must tear it
            // down on this failure path.
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(error);
        }

        Ok(())
    }
}

fn native_parent(node: &TreeNode, registry: &NativeObjectRegistry, window: HWND) -> HWND {
    node.parent
        .and_then(|parent_id| registry.get(parent_id))
        .and_then(NativeObject::content_hwnd)
        .or_else(|| {
            node.parent.and_then(|parent_id| registry.get(parent_id).map(NativeObject::hwnd))
        })
        .unwrap_or(window)
}
