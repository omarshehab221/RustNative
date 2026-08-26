//! Windows platform backend.
//!
//! The backend owns native Win32 objects and reconciles them against the
//! platform-independent Rust UI tree. Containers are native child windows,
//! which makes the Rust tree hierarchy correspond to a real Win32 hierarchy.

use std::fmt;

use framework_core::{Application, Platform};

#[derive(Debug)]
pub enum Error {
    #[cfg(windows)]
    WindowsApi { operation: &'static str, code: u32 },
    DuplicateNodeId(u64),
    UnsupportedHost,
}

#[cfg(windows)]
impl Error {
    fn windows_api(operation: &'static str) -> Self {
        Self::WindowsApi {
            operation,
            code: unsafe { windows_sys::Win32::Foundation::GetLastError() },
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(windows)]
            Self::WindowsApi { operation, code } => {
                write!(f, "Windows API call {operation} failed with error code {code}")
            }
            Self::DuplicateNodeId(id) => write!(f, "duplicate UI node id: {id}"),
            Self::UnsupportedHost => f.write_str("framework-windows is only runnable on Windows"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Default)]
pub struct WindowsPlatform;

impl WindowsPlatform {
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
impl Platform for WindowsPlatform {
    type Error = Error;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error> {
        native::run_application(application)
    }
}

#[cfg(not(windows))]
impl Platform for WindowsPlatform {
    type Error = Error;

    fn run(&mut self, _application: &mut Application) -> Result<(), Self::Error> {
        Err(Error::UnsupportedHost)
    }
}

#[cfg(windows)]
mod native {
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    use framework_core::{
        AccessibilityRole, Application, Event, IntrinsicMeasurer, KeyCode, KeyModifiers,
        LayoutEngine, NodeId, NodeKind, Point, Rect, Size, Overflow, TreeDiff, TreeNode, TreeOp,
        TreeSnapshot,
    };
    use windows_sys::Win32::Foundation::{
        GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        DrawTextW, GetDC, GetSysColorBrush, ReleaseDC, COLOR_WINDOW,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::SystemServices::{SS_LEFT, SS_NOPREFIX};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetAncestor, GetClientRect,
        GetMessageW, GetWindowLongPtrW, PostMessageW, PostQuitMessage, RegisterClassW, GetCursorPos,
        GetParent, GetWindowTextLengthW, GetWindowTextW, SendMessageW, SetParent, SetWindowLongPtrW,
        SetWindowPos, SetWindowTextW, ShowWindow,
        TranslateMessage, WindowFromPoint, BN_CLICKED, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
        CW_USEDEFAULT, GA_ROOT, GWLP_USERDATA, MSG, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOW,
        WNDCLASSW, WM_CHAR, WM_COMMAND, WM_DESTROY, WM_KEYDOWN, WM_MOUSEWHEEL, WM_NCCREATE, WM_SIZE,
        WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_CONTROLPARENT,
        WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, BS_PUSHBUTTON, ES_AUTOHSCROLL, ES_LEFT,
        WS_BORDER, EN_CHANGE, WM_APP,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetFocus, GetKeyState, SetFocus, VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT,
        VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
    };

    use super::Error;

    const WINDOW_CLASS_NAME: &str = "NativeRustFrameworkWindow";
    const CONTAINER_CLASS_NAME: &str = "NativeRustFrameworkContainer";
    const WM_FRAMEWORK_SCHEDULE: u32 = WM_APP + 1;


    #[derive(Debug, Default)]
    struct NativeObjectRegistry {
        objects: HashMap<NodeId, NativeObject>,
    }

    #[derive(Debug)]
    enum NativeObject {
        Container { viewport: HWND, content: HWND },
        Label(HWND),
        Button(HWND),
        TextInput(HWND),
    }

    impl NativeObject {
        fn hwnd(&self) -> HWND {
            match self {
                Self::Container { viewport, .. } => *viewport,
                Self::Label(hwnd) | Self::Button(hwnd) | Self::TextInput(hwnd) => *hwnd,
            }
        }

        fn content_hwnd(&self) -> Option<HWND> {
            match self {
                Self::Container { content, .. } => Some(*content),
                _ => None,
            }
        }

        fn destroy(self) {
            // SAFETY: the registry exclusively owns every HWND stored here.
            unsafe {
                DestroyWindow(self.hwnd());
            }
        }
    }

    impl NativeObjectRegistry {
        fn get(&self, id: NodeId) -> Option<&NativeObject> {
            self.objects.get(&id)
        }

        fn insert(&mut self, id: NodeId, object: NativeObject) -> Result<(), Error> {
            if self.objects.contains_key(&id) {
                return Err(Error::DuplicateNodeId(id.get()));
            }

            self.objects.insert(id, object);
            Ok(())
        }

        fn remove(&mut self, id: NodeId) -> Option<NativeObject> {
            self.objects.remove(&id)
        }

        fn id_for_hwnd(&self, hwnd: HWND) -> Option<NodeId> {
            self.objects.iter().find_map(|(id, object)| {
                (object.hwnd() == hwnd || object.content_hwnd() == Some(hwnd)).then_some(*id)
            })
        }
    }

    impl Drop for NativeObjectRegistry {
        fn drop(&mut self) {
            for (_, object) in self.objects.drain() {
                object.destroy();
            }
        }
    }

    struct WindowsIntrinsicMeasurer {
        window: HWND,
    }

    impl IntrinsicMeasurer for WindowsIntrinsicMeasurer {
        fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
            let text = text.unwrap_or_default();
            if text.is_empty() {
                return Size::new(match kind { NodeKind::Button => 24, _ => 1 }, 32);
            }

            let hdc = unsafe { GetDC(self.window) };
            if hdc.is_null() {
                return Size::new(1, 32);
            }

            let text_wide = wide(text);
            let padding = match kind {
                NodeKind::Button | NodeKind::TextInput => 24,
                NodeKind::Label | NodeKind::Column | NodeKind::Row => 0,
            };
            let available_width = max_width
                .map(|width| (width - padding).max(1))
                .unwrap_or(i32::MAX / 4);
            let mut rect = windows_sys::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: available_width,
                bottom: i32::MAX / 4,
            };
            const DT_WORDBREAK: u32 = 0x00000010;
            const DT_CALCRECT: u32 = 0x00000400;
            let flags = DT_CALCRECT | if matches!(kind, NodeKind::Label) && max_width.is_some() { DT_WORDBREAK } else { 0 };

            let measured = unsafe {
                DrawTextW(hdc, text_wide.as_ptr(), -1, &mut rect, flags)
            };
            unsafe { ReleaseDC(self.window, hdc) };

            if measured == 0 {
                return Size::new(1, 32);
            }

            let width = (rect.right - rect.left + padding).max(1);
            let height = (rect.bottom - rect.top).max(1);
            Size::new(width as u32, height as u32)
        }
    }

    #[derive(Debug)]
    struct Renderer {
        registry: NativeObjectRegistry,
        snapshot: TreeSnapshot,
        layout: HashMap<NodeId, Rect>,
        scroll_ranges: HashMap<NodeId, Size>,
        content_sizes: HashMap<NodeId, Size>,
        scroll_offsets: HashMap<NodeId, Point>,
        layout_engine: LayoutEngine,
        suppress_text_change: std::collections::HashSet<NodeId>,
    }

    impl Renderer {
        fn new() -> Self {
            Self {
                registry: NativeObjectRegistry::default(),
                snapshot: TreeSnapshot::default(),
                layout: HashMap::new(),
                scroll_ranges: HashMap::new(),
                content_sizes: HashMap::new(),
                scroll_offsets: HashMap::new(),
                layout_engine: LayoutEngine::default(),
                suppress_text_change: std::collections::HashSet::new(),
            }
        }

        fn scroll_container(&mut self, id: NodeId, delta_x: i32, delta_y: i32) {
            let Some(node) = self.snapshot.get(id) else { return; };
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
                (current.x + delta_x).clamp(0, range.width as i32),
                (current.y + delta_y).clamp(0, range.height as i32),
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
                .unwrap_or(Size::new(rect.width.max(0) as u32, rect.height.max(0) as u32));

            let width = content_size
                .width
                .max(rect.width.max(0) as u32)
                .min(i32::MAX as u32) as i32;
            let height = content_size
                .height
                .max(rect.height.max(0) as u32)
                .min(i32::MAX as u32) as i32;

            // Scrolling is a transient viewport transform. Do not rerun the
            // application component, tree reconciliation, or layout engine.
            // The content host alone moves inside the stable viewport.
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

        fn scrollable_ancestor(&self, hwnd: HWND) -> Option<NodeId> {
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
                current = unsafe { GetParent(current) };
            }
            None
        }

        fn render(&mut self, root: &framework_core::Node, window: HWND) -> Result<(), Error> {
            let next = TreeSnapshot::from_node(root).map_err(|error| match error {
                framework_core::TreeError::DuplicateNodeId(id) => Error::DuplicateNodeId(id.get()),
            })?;

            let diff = TreeDiff::between(&self.snapshot, &next);
            for operation in &diff.operations {
                self.apply_operation(operation, window)?;
            }

            let layout_invalidated = diff.invalidates_layout();
            self.snapshot = next;
            if layout_invalidated {
                self.relayout(window);
            }
            Ok(())
        }

        fn relayout(&mut self, window: HWND) {
            let mut client = RECT::default();
            unsafe {
                GetClientRect(window, &mut client);
            }

            let size = Size::new(
                (client.right - client.left).max(0) as u32,
                (client.bottom - client.top).max(0) as u32,
            );
            let measurer = WindowsIntrinsicMeasurer { window };
            let output = self.layout_engine.layout_result_with(
                &self.snapshot,
                size,
                &measurer,
                &HashMap::new(),
            );

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
                    offset.x = offset.x.clamp(0, range.width as i32);
                    offset.y = offset.y.clamp(0, range.height as i32);
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

        fn apply_operation(
            &mut self,
            operation: &TreeOp,
            window: HWND,
        ) -> Result<(), Error> {
            match operation {
                TreeOp::Insert(node) => self.insert_node(node, window),
                TreeOp::Update(node) => self.update_node(node, window),
                TreeOp::Move { id, parent, .. } => {
                    if let Some(object) = self.registry.get(*id) {
                        let parent_hwnd = parent
                            .and_then(|parent_id| self.registry.get(parent_id))
                            .and_then(NativeObject::content_hwnd)
                            .or_else(|| {
                                parent.and_then(|parent_id| self.registry.get(parent_id).map(NativeObject::hwnd))
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
                    if let Some(object) = self.registry.remove(node.id) {
                        object.destroy();
                    }
                    Ok(())
                }
            }
        }

        fn sync_accessibility_state(&self, node: &TreeNode) {
            let Some(object) = self.registry.get(node.id) else { return; };
            // Standard Win32 controls already expose their native role and visible
            // text to MSAA/UI Automation. We preserve the framework semantic model
            // here and let the later UI Automation provider map custom semantics.
            if node.accessibility.role == AccessibilityRole::Button && node.accessibility.focusable {
                unsafe {
                    SetWindowLongPtrW(object.hwnd(), windows_sys::Win32::UI::WindowsAndMessaging::GWL_STYLE,
                        GetWindowLongPtrW(object.hwnd(), windows_sys::Win32::UI::WindowsAndMessaging::GWL_STYLE) | WS_TABSTOP as isize);
                }
            }
        }

        fn insert_node(
            &mut self,
            node: &TreeNode,
            window: HWND,
        ) -> Result<(), Error> {
            let parent = native_parent(node, &self.registry, window);

            match node.kind {
                NodeKind::Column | NodeKind::Row => self.create_container(node, parent)?,
                NodeKind::Label => self.create_label(node, parent)?,
                NodeKind::Button => self.create_button(node, parent)?,
                NodeKind::TextInput => self.create_text_input(node, parent)?,
            }

            self.sync_accessibility_state(node);
            Ok(())
        }

        fn update_node(
            &mut self,
            node: &TreeNode,
            window: HWND,
        ) -> Result<(), Error> {
            let needs_replacement = match (node.kind, self.registry.get(node.id)) {
                (NodeKind::Column | NodeKind::Row, Some(NativeObject::Container { .. }))
                | (NodeKind::Label, Some(NativeObject::Label(_)))
                | (NodeKind::Button, Some(NativeObject::Button(_)))
                | (NodeKind::TextInput, Some(NativeObject::TextInput(_))) => false,
                (NodeKind::Column | NodeKind::Row, Some(_))
                | (NodeKind::Label, Some(_))
                | (NodeKind::Button, Some(_))
                | (NodeKind::TextInput, Some(_)) => true,
                (_, None) => true,
            };

            if needs_replacement {
                if let Some(object) = self.registry.remove(node.id) {
                    object.destroy();
                }
                return self.insert_node(node, window);
            }

            self.sync_accessibility_state(node);

            match node.kind {
                NodeKind::Column | NodeKind::Row => {}
                NodeKind::Label | NodeKind::Button => {
                    if let Some(object) = self.registry.get(node.id) {
                        let text = wide(node.text.as_deref().unwrap_or_default());
                        unsafe { SetWindowTextW(object.hwnd(), text.as_ptr()); }
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
                    .unwrap_or(Size::new(rect.width.max(0) as u32, rect.height.max(0) as u32));
                let width = content_size.width.max(rect.width.max(0) as u32).min(i32::MAX as u32) as i32;
                let height = content_size.height.max(rect.height.max(0) as u32).min(i32::MAX as u32) as i32;

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
                unsafe { DestroyWindow(viewport); }
                return Err(Error::windows_api("CreateWindowExW(CONTAINER_CONTENT)"));
            }

            if let Err(error) = self.registry.insert(
                node.id,
                NativeObject::Container { viewport, content },
            ) {
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
                unsafe { DestroyWindow(hwnd) };
                return Err(error);
            }

            Ok(())
        }

        fn create_button(&mut self, node: &TreeNode, parent: HWND) -> Result<(), Error> {
            let text = wide(node.text.as_deref().unwrap_or_default());
            let class = wide("BUTTON");
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
                unsafe { DestroyWindow(hwnd) };
                return Err(error);
            }

            Ok(())
        }
        fn create_text_input(&mut self, node: &TreeNode, parent: HWND) -> Result<(), Error> {
            let value = wide(node.text.as_deref().unwrap_or_default());
            let class = wide("EDIT");
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    value.as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
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
                unsafe { DestroyWindow(hwnd); }
                return Err(error);
            }

            Ok(())
        }

    }

    fn native_parent(node: &TreeNode, registry: &NativeObjectRegistry, window: HWND) -> HWND {
        node.parent
            .and_then(|parent_id| registry.get(parent_id))
            .and_then(NativeObject::content_hwnd)
            .or_else(|| node.parent.and_then(|parent_id| registry.get(parent_id).map(NativeObject::hwnd)))
            .unwrap_or(window)
    }

    struct Runtime {
        application: *mut Application,
        renderer: Renderer,
        window: HWND,
        focused: Option<NodeId>,
        error: Option<Error>,
    }

    impl Runtime {
        fn render(&mut self) -> Result<(), Error> {
            // SAFETY: `application` points to the mutable Application borrowed by
            // `WindowsPlatform::run` and remains valid for this event loop.
            let application = unsafe { &*self.application };
            let tree = application.view();
            self.renderer.render(&tree, self.window)
        }

        fn relayout(&mut self) {
            self.renderer.relayout(self.window);
        }

        fn dispatch(&mut self, event: Event) -> Result<(), Error> {
            // SAFETY: see `render`; the event loop has exclusive access to the
            // application while it is running.
            let application = unsafe { &mut *self.application };
            let handled = application.dispatch(event);

            // ComponentTree::dispatch updates component state and rebuilds the
            // framework tree. Reconcile that new tree back into native controls
            // immediately so the visible UI stays in sync with Rust state.
            if handled {
                self.render()?;
            }

            Ok(())
        }

        fn pump_tasks(&mut self) -> Result<(), Error> {
            // SAFETY: the runtime owns the application for the duration of the
            // native event loop.
            let application = unsafe { &mut *self.application };
            if application.pump_tasks() {
                self.render()?;
            }
            Ok(())
        }
    }

    pub(super) fn run_application(application: &mut Application) -> Result<(), Error> {
        let instance = module_instance();
        register_window_classes(instance)?;

        let mut runtime = Box::new(Runtime {
            application: application as *mut Application,
            renderer: Renderer::new(),
            window: null_mut(),
            focused: None,
            error: None,
        });

        let title = wide(application.window().title());
        let class = wide(WINDOW_CLASS_NAME);
        let size = application.window().size();
        let runtime_ptr: *mut Runtime = &mut *runtime;

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                size.width as i32,
                size.height as i32,
                null_mut(),
                null_mut(),
                instance,
                runtime_ptr.cast(),
            )
        };

        if hwnd.is_null() {
            return Err(Error::windows_api("CreateWindowExW(top-level)"));
        }

        runtime.window = hwnd;

        let wake_target = hwnd as usize;
        application.scheduler().set_waker(std::sync::Arc::new(move || {
            let hwnd = wake_target as HWND;
            unsafe { PostMessageW(hwnd, WM_FRAMEWORK_SCHEDULE, 0, 0); }
        }));

        runtime.render()?;

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
        }

        run_message_loop(&mut runtime)?;

        if let Some(error) = runtime.error.take() {
            return Err(error);
        }

        Ok(())
    }

    fn key_code(vkey: u32) -> KeyCode {
        match vkey as u16 {
            VK_RETURN => KeyCode::Enter,
            VK_SPACE => KeyCode::Space,
            VK_TAB => KeyCode::Tab,
            VK_ESCAPE => KeyCode::Escape,
            VK_BACK => KeyCode::Backspace,
            VK_LEFT => KeyCode::ArrowLeft,
            VK_RIGHT => KeyCode::ArrowRight,
            VK_UP => KeyCode::ArrowUp,
            VK_DOWN => KeyCode::ArrowDown,
            value if (0x30..=0x5A).contains(&value) => KeyCode::Character(char::from_u32(value as u32).unwrap_or('?')),
            value => KeyCode::Unknown(value as u32),
        }
    }

    fn modifiers() -> KeyModifiers {
        let shift = unsafe { GetKeyState(VK_SHIFT as i32) } & i16::MIN != 0;
        let ctrl = unsafe { GetKeyState(0x11) } & i16::MIN != 0;
        let alt = unsafe { GetKeyState(0x12) } & i16::MIN != 0;
        KeyModifiers { shift, ctrl, alt }
    }

    fn focused_node(runtime: &Runtime) -> Option<NodeId> {
        let focus = unsafe { GetFocus() };
        if focus.is_null() { None } else { runtime.renderer.registry.id_for_hwnd(focus) }
    }

    fn focus_next(runtime: &mut Runtime, backwards: bool) {
        let focusable = runtime
            .renderer
            .snapshot
            .ordered_nodes()
            .into_iter()
            .filter(|node| node.accessibility.focusable && runtime.renderer.registry.get(node.id).is_some())
            .collect::<Vec<_>>();
        if focusable.is_empty() { return; }

        let current = runtime.focused.and_then(|id| focusable.iter().position(|node| node.id == id));
        let next_index = match current {
            Some(index) if backwards => if index == 0 { focusable.len() - 1 } else { index - 1 },
            Some(index) => (index + 1) % focusable.len(),
            None if backwards => focusable.len() - 1,
            None => 0,
        };

        let next_id = focusable[next_index].id;
        if let Some(object) = runtime.renderer.registry.get(next_id) {
            unsafe { SetFocus(object.hwnd()); }
        }
    }

    fn sync_focus(runtime: &mut Runtime) {
        let next = focused_node(runtime);
        if next == runtime.focused { return; }

        if let Some(previous) = runtime.focused.take() {
            if let Err(error) = runtime.dispatch(Event::FocusLost { target: previous }) {
                runtime.error = Some(error);
                return;
            }
        }

        runtime.focused = next;
        if let Some(current) = next {
            if let Err(error) = runtime.dispatch(Event::FocusGained { target: current }) {
                runtime.error = Some(error);
            }
        }
    }

    fn run_message_loop(runtime: &mut Runtime) -> Result<(), Error> {
        let mut message = MSG::default();

        loop {
            let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };

            if result == -1 {
                return Err(Error::windows_api("GetMessageW"));
            }

            if result == 0 {
                break;
            }

            if message.message == WM_FRAMEWORK_SCHEDULE {
                runtime.pump_tasks()?;
                continue;
            }

            if message.message == WM_MOUSEWHEEL {
                let mut point = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
                unsafe { GetCursorPos(&mut point); }
                let hovered = unsafe { WindowFromPoint(point) };
                if let Some(id) = runtime.renderer.scrollable_ancestor(hovered) {
                    let delta = ((message.wParam >> 16) & 0xffff) as u16 as i16;
                    runtime.renderer.scroll_container(
                        id,
                        0,
                        -(i32::from(delta) / 3).clamp(-120, 120),
                    );
                    continue;
                }
            }

            if message.message == WM_KEYDOWN {
                let key = key_code(message.wParam as u32);
                if key == KeyCode::Tab {
                    let backwards = modifiers().shift;
                    focus_next(runtime, backwards);
                    sync_focus(runtime);
                    continue;
                }

                let target = focused_node(runtime);
                let event = Event::KeyDown {
                    target,
                    key,
                    modifiers: modifiers(),
                };
                if let Err(error) = runtime.dispatch(event) {
                    runtime.error = Some(error);
                    unsafe { PostQuitMessage(1); }
                    continue;
                }
            } else if message.message == WM_CHAR {
                let target = focused_node(runtime);
                let native_text_input = target
                    .and_then(|id| runtime.renderer.registry.get(id))
                    .is_some_and(|object| matches!(object, NativeObject::TextInput(_)));

                if !native_text_input {
                    if let Some(character) = char::from_u32(message.wParam as u32) {
                        if !character.is_control() {
                            if let Err(error) = runtime.dispatch(Event::TextInput {
                                target,
                                text: character.to_string(),
                            }) {
                                runtime.error = Some(error);
                                unsafe { PostQuitMessage(1); }
                                continue;
                            }
                        }
                    }
                }
            }

            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            sync_focus(runtime);
            if runtime.error.is_some() { break; }
        }

        Ok(())
    }

    fn register_window_classes(instance: HINSTANCE) -> Result<(), Error> {
        register_window_class(instance, WINDOW_CLASS_NAME, window_proc)?;
        register_window_class(instance, CONTAINER_CLASS_NAME, container_proc)
    }

    fn register_window_class(
        instance: HINSTANCE,
        class_name: &str,
        window_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    ) -> Result<(), Error> {
        let class_name = wide(class_name);

        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };

        let atom = unsafe { RegisterClassW(&class) };
        if atom == 0 {
            let error = unsafe { GetLastError() };
            const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;
            if error != ERROR_CLASS_ALREADY_EXISTS {
                return Err(Error::WindowsApi {
                    operation: "RegisterClassW",
                    code: error,
                });
            }
        }

        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create = lparam as *const CREATESTRUCTW;
                if create.is_null() {
                    return 0;
                }

                let runtime_ptr = unsafe { (*create).lpCreateParams } as *mut Runtime;
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, runtime_ptr as isize);
                }
                1
            }
            WM_COMMAND => {
                let notification_code = ((wparam >> 16) & 0xffff) as u32;
                let control = lparam as HWND;
                let runtime_ptr =
                    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Runtime;

                if !runtime_ptr.is_null() && !control.is_null() {
                    let runtime = unsafe { &mut *runtime_ptr };
                    if let Some(id) = runtime.renderer.registry.id_for_hwnd(control) {
                        if notification_code == BN_CLICKED {
                            if let Err(error) = runtime.dispatch(Event::Click { target: id }) {
                                runtime.error = Some(error);
                                unsafe { PostQuitMessage(1) };
                            }
                        } else if notification_code == EN_CHANGE {
                            if runtime.renderer.suppress_text_change.remove(&id) {
                                return 0;
                            }

                            let is_text_input = runtime
                                .renderer
                                .registry
                                .get(id)
                                .is_some_and(|object| matches!(object, NativeObject::TextInput(_)));
                            if is_text_input {
                                let value = window_text(control);
                                if let Err(error) = runtime.dispatch(Event::TextChanged {
                                    target: id,
                                    value,
                                }) {
                                    runtime.error = Some(error);
                                    unsafe { PostQuitMessage(1) };
                                }
                            }
                        }
                    }
                }
                0
            }
            WM_SIZE => {
                let runtime_ptr =
                    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Runtime;
                if !runtime_ptr.is_null() {
                    unsafe { &mut *runtime_ptr }.relayout();
                }
                0
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    unsafe extern "system" fn container_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_COMMAND => {
                let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
                if !root.is_null() {
                    unsafe {
                        SendMessageW(root, WM_COMMAND, wparam, lparam);
                    }
                    0
                } else {
                    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
                }
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn module_instance() -> HINSTANCE {
        unsafe { GetModuleHandleW(null()) }
    }

    fn window_text(hwnd: HWND) -> String {
        let length = unsafe { GetWindowTextLengthW(hwnd) };
        if length <= 0 {
            return String::new();
        }

        let mut buffer = vec![0u16; length as usize + 1];
        let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..copied as usize])
    }

    fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(once(0)).collect()
    }
}
