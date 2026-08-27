//! Windows platform backend.
//!
//! The backend owns native Win32 objects and reconciles them against the
//! platform-independent Rust UI tree. Containers are native child windows,
//! which makes the Rust tree hierarchy correspond to a real Win32 hierarchy.

use std::fmt;

use framework_core::{Application, Capability, Platform, PlatformCapabilities};

/// Native Win32 clipboard service. Applications opt into it by injecting an
/// `Arc<WindowsClipboard>` into `framework_core::Services`.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsClipboard;

/// Native shell integration. `open_url` delegates to the user's registered
/// browser and returns an explicit error when Windows rejects the request.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsSystem;

#[cfg(windows)]
impl framework_core::SystemService for WindowsSystem {
    fn open_url(&self, url: String) -> framework_core::ServiceFuture<()> {
        Box::pin(async move {
            use windows_sys::Win32::UI::Shell::ShellExecuteW;
            use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

            let file = wide_string(&url);
            let result = unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    file.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            };
            if (result as isize) <= 32 {
                return Err(framework_core::ServiceError::new(
                    "ShellExecuteW failed to open URL",
                ));
            }
            Ok(())
        })
    }

    fn notify(&self, title: String, body: String) -> framework_core::ServiceFuture<()> {
        Box::pin(async move {
            use windows_sys::Win32::UI::Shell::{
                NIF_ICON, NIF_INFO, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::{IDI_INFORMATION, LoadIconW};

            // A one-shot notification still needs a taskbar icon: Windows
            // routes the balloon through the icon it identifies by
            // (hWnd, uID), and tears it back down immediately after so no
            // permanent tray icon is left behind.
            let icon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_INFORMATION) };

            let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
            data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = std::ptr::null_mut();
            data.uID = 1;
            data.uFlags = NIF_ICON | NIF_TIP | NIF_INFO;
            data.hIcon = icon;
            data.dwInfoFlags = NIIF_INFO;
            copy_into_wide_buffer(&title, &mut data.szInfoTitle);
            copy_into_wide_buffer(&body, &mut data.szInfo);
            copy_into_wide_buffer(&title, &mut data.szTip);

            if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
                return Err(framework_core::ServiceError::new(
                    "Shell_NotifyIconW(NIM_ADD) failed to post the notification",
                ));
            }
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &data);
            }
            Ok(())
        })
    }
}

/// Copies `text` into a fixed-size `NOTIFYICONDATAW` wide-character field,
/// truncating (and always null-terminating) when it doesn't fit.
#[cfg(windows)]
fn copy_into_wide_buffer(text: &str, buffer: &mut [u16]) {
    if buffer.is_empty() {
        return;
    }
    let encoded: Vec<u16> = text.encode_utf16().collect();
    let usable = (buffer.len() - 1).min(encoded.len());
    buffer[..usable].copy_from_slice(&encoded[..usable]);
    buffer[usable] = 0;
}

/// Native Win32 file dialogs (open, save, and folder pickers), realized with
/// the classic common-dialog APIs rather than the newer `IFileDialog` COM
/// interface: they need no COM activation factory registration and are
/// stable across every supported Windows version.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsFileDialogs;

#[cfg(windows)]
impl framework_core::FileDialogService for WindowsFileDialogs {
    fn show(
        &self,
        request: framework_core::FileDialogRequest,
    ) -> framework_core::ServiceFuture<Option<String>> {
        Box::pin(async move { show_file_dialog(request) })
    }
}

#[cfg(windows)]
fn show_file_dialog(
    request: framework_core::FileDialogRequest,
) -> Result<Option<String>, framework_core::ServiceError> {
    use framework_core::FileDialogKind;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    // The common dialogs use COM-backed shell extensions (thumbnails, recent
    // places) internally. Each service call runs on its own dedicated
    // framework worker thread (see `Scheduler::spawn`), so initializing a
    // fresh apartment here is both safe and necessary; `CoUninitialize` pairs
    // with it before this call returns.
    let com_initialized =
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0;

    let result = match request.kind {
        FileDialogKind::OpenFile => show_open_or_save(&request, true),
        FileDialogKind::SaveFile => show_open_or_save(&request, false),
        FileDialogKind::PickFolder => show_folder_picker(&request),
    };

    if com_initialized {
        unsafe {
            CoUninitialize();
        }
    }
    result
}

#[cfg(windows)]
fn show_open_or_save(
    request: &framework_core::FileDialogRequest,
    is_open: bool,
) -> Result<Option<String>, framework_core::ServiceError> {
    use windows_sys::Win32::Foundation::MAX_PATH;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY,
        OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    let filter = build_filter_string(&request.filters);
    let title = request.title.as_deref().map(wide_string);
    let mut file_buffer = vec![0u16; MAX_PATH as usize * 4];

    let flags = if is_open {
        OFN_EXPLORER | OFN_HIDEREADONLY | OFN_NOCHANGEDIR | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST
    } else {
        OFN_EXPLORER | OFN_HIDEREADONLY | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT
    };

    let mut config = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: if filter.is_empty() {
            std::ptr::null()
        } else {
            filter.as_ptr()
        },
        nFilterIndex: if filter.is_empty() { 0 } else { 1 },
        lpstrFile: file_buffer.as_mut_ptr(),
        nMaxFile: file_buffer.len() as u32,
        lpstrTitle: title
            .as_ref()
            .map(|title| title.as_ptr())
            .unwrap_or(std::ptr::null()),
        Flags: flags,
        ..Default::default()
    };

    let succeeded = unsafe {
        if is_open {
            GetOpenFileNameW(&mut config)
        } else {
            GetSaveFileNameW(&mut config)
        }
    };

    if succeeded == 0 {
        // A zero result with no extended error means the person cancelled
        // the dialog; that is not a service failure.
        return Ok(None);
    }

    let selected_len = file_buffer.iter().position(|&c| c == 0).unwrap_or(0);
    Ok(Some(String::from_utf16_lossy(&file_buffer[..selected_len])))
}

#[cfg(windows)]
fn show_folder_picker(
    request: &framework_core::FileDialogRequest,
) -> Result<Option<String>, framework_core::ServiceError> {
    use windows_sys::Win32::Foundation::MAX_PATH;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW,
        SHGetPathFromIDListW,
    };

    let title = request.title.as_deref().map(wide_string);
    let mut display_name = vec![0u16; MAX_PATH as usize];
    let info = BROWSEINFOW {
        pszDisplayName: display_name.as_mut_ptr(),
        lpszTitle: title
            .as_ref()
            .map(|title| title.as_ptr())
            .unwrap_or(std::ptr::null()),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        ..Default::default()
    };

    let item_list = unsafe { SHBrowseForFolderW(&info) };
    if item_list.is_null() {
        // The person cancelled the picker.
        return Ok(None);
    }

    let mut path = vec![0u16; MAX_PATH as usize];
    let resolved = unsafe { SHGetPathFromIDListW(item_list, path.as_mut_ptr()) };
    unsafe {
        CoTaskMemFree(item_list as *const std::ffi::c_void);
    }

    if resolved == 0 {
        return Err(framework_core::ServiceError::new(
            "SHGetPathFromIDListW failed to resolve the selected folder",
        ));
    }

    let selected_len = path.iter().position(|&c| c == 0).unwrap_or(0);
    Ok(Some(String::from_utf16_lossy(&path[..selected_len])))
}

/// Builds a Win32 `lpstrFilter` buffer: null-separated description/pattern
/// pairs, terminated by an extra null. An empty `filters` list yields an
/// empty buffer, which callers translate into a null pointer so the dialog
/// falls back to its default "All Files" filter.
#[cfg(windows)]
fn build_filter_string(filters: &[(String, Vec<String>)]) -> Vec<u16> {
    if filters.is_empty() {
        return Vec::new();
    }
    let mut buffer = String::new();
    for (label, patterns) in filters {
        buffer.push_str(label);
        buffer.push('\0');
        if patterns.is_empty() {
            buffer.push_str("*.*");
        } else {
            buffer.push_str(&patterns.join(";"));
        }
        buffer.push('\0');
    }
    buffer.push('\0');
    buffer.encode_utf16().collect()
}

#[cfg(windows)]
fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalFree(
        memory: windows_sys::Win32::Foundation::HGLOBAL,
    ) -> windows_sys::Win32::Foundation::HGLOBAL;
}

#[cfg(windows)]
impl framework_core::ClipboardService for WindowsClipboard {
    fn read_text(&self) -> framework_core::ServiceFuture<Option<String>> {
        Box::pin(async move {
            use windows_sys::Win32::System::DataExchange::{
                CloseClipboard, GetClipboardData, OpenClipboard,
            };
            use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

            const CF_UNICODETEXT: u32 = 13;
            // The clipboard API permits a null owner for non-window-bound reads.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
                return Err(framework_core::ServiceError::new("OpenClipboard failed"));
            }
            struct ClipboardGuard;
            impl Drop for ClipboardGuard {
                fn drop(&mut self) {
                    unsafe {
                        CloseClipboard();
                    }
                }
            }
            let _guard = ClipboardGuard;
            let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
            if handle.is_null() {
                return Ok(None);
            }
            let value = unsafe { GlobalLock(handle) } as *const u16;
            if value.is_null() {
                return Err(framework_core::ServiceError::new(
                    "GlobalLock clipboard data failed",
                ));
            }
            let mut length = 0usize;
            while unsafe { *value.add(length) } != 0 {
                length += 1;
            }
            let text =
                String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value, length) });
            unsafe {
                GlobalUnlock(handle);
            }
            Ok(Some(text))
        })
    }

    fn write_text(&self, text: String) -> framework_core::ServiceFuture<()> {
        Box::pin(async move {
            use windows_sys::Win32::System::DataExchange::{
                CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
            };
            use windows_sys::Win32::System::Memory::{
                GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
            };

            const CF_UNICODETEXT: u32 = 13;
            let mut utf16 = text.encode_utf16().collect::<Vec<_>>();
            utf16.push(0);
            if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
                return Err(framework_core::ServiceError::new("OpenClipboard failed"));
            }
            struct ClipboardGuard;
            impl Drop for ClipboardGuard {
                fn drop(&mut self) {
                    unsafe {
                        CloseClipboard();
                    }
                }
            }
            let _guard = ClipboardGuard;
            if unsafe { EmptyClipboard() } == 0 {
                return Err(framework_core::ServiceError::new("EmptyClipboard failed"));
            }
            let memory =
                unsafe { GlobalAlloc(GMEM_MOVEABLE, utf16.len() * std::mem::size_of::<u16>()) };
            if memory.is_null() {
                return Err(framework_core::ServiceError::new(
                    "GlobalAlloc clipboard data failed",
                ));
            }
            let destination = unsafe { GlobalLock(memory) } as *mut u16;
            if destination.is_null() {
                unsafe {
                    GlobalFree(memory);
                }
                return Err(framework_core::ServiceError::new(
                    "GlobalLock clipboard data failed",
                ));
            }
            unsafe {
                std::ptr::copy_nonoverlapping(utf16.as_ptr(), destination, utf16.len());
                GlobalUnlock(memory);
            }
            if unsafe { SetClipboardData(CF_UNICODETEXT, memory) }.is_null() {
                unsafe {
                    GlobalFree(memory);
                }
                return Err(framework_core::ServiceError::new("SetClipboardData failed"));
            }
            Ok(())
        })
    }
}

#[derive(Debug)]
pub enum Error {
    #[cfg(windows)]
    WindowsApi {
        operation: &'static str,
        code: u32,
    },
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
                write!(
                    f,
                    "Windows API call {operation} failed with error code {code}"
                )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
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

#[cfg(windows)]
mod native {
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    use framework_core::{
        AccessibilityRole, Application, Color, Event, IntrinsicMeasurer, KeyCode, KeyModifiers,
        LayoutEngine, MenuBar, MenuItem, NodeId, NodeKind, Overflow, Point, Rect, Size, Theme,
        TreeDiff, TreeNode, TreeOp, TreeSnapshot, Typography, VisualStyle, WindowId,
    };
    use windows_sys::Win32::Foundation::{
        COLORREF, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        CLIP_DEFAULT_PRECIS, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontIndirectW,
        CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DeleteObject,
        DrawTextW, FF_DONTCARE, FillRect, GetDC, GetSysColor, GetSysColorBrush, HBRUSH, HDC,
        HFONT, HGDIOBJ, InvalidateRect, LOGFONTW, OUT_DEFAULT_PRECIS, ReleaseDC, SetBkColor,
        SetBkMode, SetTextColor, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::SystemServices::{SS_LEFT, SS_NOPREFIX};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetFocus, GetKeyState, SetFocus, VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT,
        VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, BN_CLICKED, BS_PUSHBUTTON, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
        CW_USEDEFAULT, CreateMenu, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyWindow, DispatchMessageW, EN_CHANGE, ES_AUTOHSCROLL, ES_LEFT, GA_ROOT,
        GWLP_USERDATA, GetAncestor, GetClientRect, GetCursorPos, GetMessageW, GetParent,
        GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, MF_CHECKED, MF_GRAYED,
        MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
        SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetMenu, SetParent,
        SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WM_APP,
        WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC,
        WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_MOUSEWHEEL, WM_MOVE, WM_NCCREATE, WM_SETFONT,
        WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WindowFromPoint,
    };

    use super::Error;

    const WINDOW_CLASS_NAME: &str = "NativeRustFrameworkWindow";
    const CONTAINER_CLASS_NAME: &str = "NativeRustFrameworkContainer";
    const WM_FRAMEWORK_SCHEDULE: u32 = WM_APP + 1;

    #[link(name = "user32")]
    unsafe extern "system" {
        /// Enables or disables a top-level owner while a modal child is open.
        fn EnableWindow(hwnd: HWND, enable: i32) -> i32;
    }

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
                return Size::new(
                    match kind {
                        NodeKind::Button => 24,
                        _ => 1,
                    },
                    32,
                );
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
            let flags = DT_CALCRECT
                | if matches!(kind, NodeKind::Label) && max_width.is_some() {
                    DT_WORDBREAK
                } else {
                    0
                };

            let measured = unsafe { DrawTextW(hdc, text_wide.as_ptr(), -1, &mut rect, flags) };
            unsafe { ReleaseDC(self.window, hdc) };

            if measured == 0 {
                return Size::new(1, 32);
            }

            let width = (rect.right - rect.left + padding).max(1);
            let height = (rect.bottom - rect.top).max(1);
            Size::new(width as u32, height as u32)
        }
    }

    /// Cached GDI resources realizing one node's fully resolved
    /// `VisualStyle`. Kept alive for as long as the node exists so
    /// `WM_CTLCOLOR*`/`WM_ERASEBKGND` handlers can hand back a stable brush
    /// on every repaint, and explicitly torn down (never left as a bare GDI
    /// handle leak) when the node's style changes or the node is removed.
    #[derive(Debug)]
    struct ControlStyle {
        foreground: COLORREF,
        background: COLORREF,
        background_brush: HBRUSH,
        font: HFONT,
    }

    impl ControlStyle {
        /// Realizes a node's already theme-resolved style (see
        /// `TreeSnapshot::from_node_with_theme`) as GDI resources. Any
        /// component missing from the resolved style — which should only
        /// happen for a snapshot that skipped theme resolution — falls back
        /// to the corresponding system color so a control is never left
        /// unpainted.
        fn resolve(style: &VisualStyle) -> Self {
            let foreground = style
                .foreground
                .map(color_ref)
                .unwrap_or_else(|| unsafe { GetSysColor(COLOR_WINDOWTEXT) });
            let background = style
                .background
                .map(color_ref)
                .unwrap_or_else(|| unsafe { GetSysColor(COLOR_WINDOW) });
            let background_brush = unsafe { CreateSolidBrush(background) };
            let font = style.typography.as_ref().map(create_font).unwrap_or(null_mut());

            Self {
                foreground,
                background,
                background_brush,
                font,
            }
        }
    }

    impl Drop for ControlStyle {
        fn drop(&mut self) {
            unsafe {
                if !self.background_brush.is_null() {
                    DeleteObject(self.background_brush as HGDIOBJ);
                }
                if !self.font.is_null() {
                    DeleteObject(self.font as HGDIOBJ);
                }
            }
        }
    }

    fn color_ref(color: Color) -> COLORREF {
        color.red as u32 | (color.green as u32) << 8 | (color.blue as u32) << 16
    }

    fn create_font(typography: &Typography) -> HFONT {
        let mut face_name = [0u16; 32];
        let encoded: Vec<u16> = typography.family.encode_utf16().take(31).collect();
        face_name[..encoded.len()].copy_from_slice(&encoded);

        // A negative `lfHeight` asks GDI for a character height in logical
        // (pixel, at the default 96 DPI this framework currently assumes)
        // units rather than a cell height, which is what the framework's
        // `Typography::size` is meant to represent.
        let logfont = LOGFONTW {
            lfHeight: -(typography.size as i32),
            lfWidth: 0,
            lfEscapement: 0,
            lfOrientation: 0,
            lfWeight: typography.weight as i32,
            lfItalic: 0,
            lfUnderline: 0,
            lfStrikeOut: 0,
            lfCharSet: DEFAULT_CHARSET,
            lfOutPrecision: OUT_DEFAULT_PRECIS,
            lfClipPrecision: CLIP_DEFAULT_PRECIS,
            lfQuality: DEFAULT_QUALITY,
            lfPitchAndFamily: DEFAULT_PITCH | FF_DONTCARE,
            lfFaceName: face_name,
        };

        unsafe { CreateFontIndirectW(&logfont) }
    }

    #[derive(Debug)]
    struct Renderer {
        registry: NativeObjectRegistry,
        styles: HashMap<NodeId, ControlStyle>,
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
                styles: HashMap::new(),
                snapshot: TreeSnapshot::default(),
                layout: HashMap::new(),
                scroll_ranges: HashMap::new(),
                content_sizes: HashMap::new(),
                scroll_offsets: HashMap::new(),
                layout_engine: LayoutEngine,
                suppress_text_change: std::collections::HashSet::new(),
            }
        }

        fn scroll_container(&mut self, id: NodeId, delta_x: i32, delta_y: i32) {
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

            let range = self
                .scroll_ranges
                .get(&id)
                .copied()
                .unwrap_or(Size::new(0, 0));
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
            let content_size = self.content_sizes.get(&id).copied().unwrap_or(Size::new(
                rect.width.max(0) as u32,
                rect.height.max(0) as u32,
            ));

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

        fn render(&mut self, root: &framework_core::Node, window: HWND, theme: &Theme) -> Result<(), Error> {
            let next = TreeSnapshot::from_node_with_theme(root, theme).map_err(|error| match error {
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
            self.scroll_offsets
                .retain(|id, _| self.snapshot.contains(*id));
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
            // Standard Win32 controls already expose their native role and visible
            // text to MSAA/UI Automation. We preserve the framework semantic model
            // here and let the later UI Automation provider map custom semantics.
            if node.accessibility.role == AccessibilityRole::Button && node.accessibility.focusable
            {
                unsafe {
                    SetWindowLongPtrW(
                        object.hwnd(),
                        windows_sys::Win32::UI::WindowsAndMessaging::GWL_STYLE,
                        GetWindowLongPtrW(
                            object.hwnd(),
                            windows_sys::Win32::UI::WindowsAndMessaging::GWL_STYLE,
                        ) | WS_TABSTOP as isize,
                    );
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

            let style = ControlStyle::resolve(&node.visual_style);

            if !style.font.is_null() {
                unsafe {
                    SendMessageW(hwnd, WM_SETFONT, style.font as WPARAM, 1);
                    if let Some(content_hwnd) = content_hwnd {
                        SendMessageW(content_hwnd, WM_SETFONT, style.font as WPARAM, 1);
                    }
                }
            }

            match node.kind {
                NodeKind::Column | NodeKind::Row => {
                    unsafe {
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, style.background as isize);
                        if let Some(content_hwnd) = content_hwnd {
                            SetWindowLongPtrW(content_hwnd, GWLP_USERDATA, style.background as isize);
                        }
                    }
                }
                NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {}
            }

            unsafe {
                EnableWindow(hwnd, if node.disabled { 0 } else { 1 });
                InvalidateRect(hwnd, null(), 1);
            }

            self.styles.insert(node.id, style);
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
            self.apply_control_style(node);

            match node.kind {
                NodeKind::Column | NodeKind::Row => {}
                NodeKind::Label | NodeKind::Button => {
                    if let Some(object) = self.registry.get(node.id) {
                        let text = wide(node.text.as_deref().unwrap_or_default());
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
                let content_size = self.content_sizes.get(&id).copied().unwrap_or(Size::new(
                    rect.width.max(0) as u32,
                    rect.height.max(0) as u32,
                ));
                let width = content_size
                    .width
                    .max(rect.width.max(0) as u32)
                    .min(i32::MAX as u32) as i32;
                let height = content_size
                    .height
                    .max(rect.height.max(0) as u32)
                    .min(i32::MAX as u32) as i32;

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
                unsafe {
                    DestroyWindow(viewport);
                }
                return Err(Error::windows_api("CreateWindowExW(CONTAINER_CONTENT)"));
            }

            if let Err(error) = self
                .registry
                .insert(node.id, NativeObject::Container { viewport, content })
            {
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
                node.parent
                    .and_then(|parent_id| registry.get(parent_id).map(NativeObject::hwnd))
            })
            .unwrap_or(window)
    }

    struct Runtime {
        application: *mut Application,
        window_id: WindowId,
        modal_parent: HWND,
        renderer: Renderer,
        window: HWND,
        focused: Option<NodeId>,
        error: Option<Error>,
        registry: *mut WindowRegistry,
        menu_commands: HashMap<u16, NodeId>,
        destroyed: bool,
    }

    impl Runtime {
        fn render(&mut self) -> Result<(), Error> {
            // SAFETY: `application` points to the mutable Application borrowed by
            // `WindowsPlatform::run` and remains valid for this event loop.
            let application = unsafe { &*self.application };
            let Some(tree) = application.view_for(self.window_id) else {
                return Ok(());
            };
            self.renderer.render(&tree, self.window, application.theme())
        }

        fn relayout(&mut self) {
            self.renderer.relayout(self.window);
        }

        fn dispatch(&mut self, event: Event) -> Result<(), Error> {
            // SAFETY: see `render`; the event loop has exclusive access to the
            // application while it is running.
            let application = unsafe { &mut *self.application };
            let handled = application.dispatch_to_window(self.window_id, event);

            // ComponentTree::dispatch updates component state and rebuilds the
            // framework tree. Reconcile that new tree back into native controls
            // immediately so the visible UI stays in sync with Rust state.
            if handled {
                self.render()?;
            }

            // A component may have queued a window-open or window-close
            // request while handling that event (see `ComponentContext::windows`).
            // Pick up any resulting change to the application's window set.
            self.sync_windows()
        }

        fn pump_tasks(&mut self) -> Result<(), Error> {
            // SAFETY: the runtime owns the application for the duration of the
            // native event loop.
            let application = unsafe { &mut *self.application };
            if application.pump_tasks_for(self.window_id) {
                self.render()?;
            }
            self.sync_windows()
        }

        /// Picks up any window opened or closed since the last sync. See
        /// `WindowRegistry::sync` for how each side is realized.
        fn sync_windows(&mut self) -> Result<(), Error> {
            if self.registry.is_null() {
                return Ok(());
            }
            // SAFETY: `registry` outlives every `Runtime` it owns; see
            // `run_application`.
            unsafe { &mut *self.registry }.sync()
        }
    }

    /// Owns every top-level `Runtime` for one `run_application` call and
    /// keeps their native windows in sync with `Application::window_ids()`
    /// as components open and close windows at runtime.
    ///
    /// Runtimes are intentionally never removed from `runtimes` while the
    /// message loop is running, even after their native window is destroyed:
    /// a `Runtime` can be mid-dispatch (and so borrowed by a caller further
    /// up the call stack) at the exact moment its own window is asked to
    /// close, and removing it from this map would drop — and free — memory
    /// that caller still holds a reference to. Instead, closing a window
    /// posts it a `WM_CLOSE` (see `sync`) and lets its own, already-correct
    /// `WM_CLOSE`/`WM_DESTROY` handling in `window_proc` tear it down on a
    /// later, unnested turn of the message loop. Every `Runtime` — destroyed
    /// or not — is finally dropped when `run_application` returns.
    struct WindowRegistry {
        application: *mut Application,
        runtimes: HashMap<WindowId, Box<Runtime>>,
        // Reentrancy guard, belt-and-braces alongside registering in
        // `runtimes` as early as possible in `create_window`: covers the
        // (believed unreachable without `WS_VISIBLE`, but unverified on a
        // real Windows compiler — see BUILD_STATUS.md) case where a native
        // message is delivered synchronously even earlier than that, from
        // inside `CreateWindowExW` itself.
        creating: std::collections::HashSet<WindowId>,
    }

    impl WindowRegistry {
        fn sync(&mut self) -> Result<(), Error> {
            // SAFETY: `application` is the same pointer every `Runtime` here
            // already dereferences to dispatch events.
            let application = unsafe { &*self.application };
            let desired = application.window_ids();

            for (id, runtime) in self.runtimes.iter_mut() {
                if runtime.destroyed || desired.contains(id) {
                    continue;
                }
                // Deferred: see the type-level doc comment on why this must
                // not destroy the window inline.
                runtime.destroyed = true;
                unsafe {
                    PostMessageW(runtime.window, WM_CLOSE, 0, 0);
                }
            }

            for id in desired {
                if !self.runtimes.contains_key(&id) && !self.creating.contains(&id) {
                    self.create_window(id)?;
                }
            }
            Ok(())
        }

        fn create_window(&mut self, id: WindowId) -> Result<(), Error> {
            // See the field doc comment on `creating`: this, together with
            // registering in `runtimes` as early as possible below, is what
            // stops a window from being created twice — which without this
            // guard is not a cosmetic bug but unbounded native window
            // creation, since each spurious window creation is itself
            // exactly the kind of event that triggers another `sync()`.
            if !self.creating.insert(id) {
                return Ok(());
            }
            let result = self.create_window_once(id);
            self.creating.remove(&id);
            result
        }

        fn create_window_once(&mut self, id: WindowId) -> Result<(), Error> {
            // SAFETY: see `sync`.
            let application = unsafe { &*self.application };
            let Some(definition) = application.window_for(id) else {
                return Ok(());
            };
            let title = wide(definition.title());
            let size = definition.size();
            let menu = definition.menu().cloned();
            let owner = application
                .window_state(id)
                .and_then(|state| state.modal_parent)
                .and_then(|parent_id| self.runtimes.get(&parent_id))
                .map(|runtime| runtime.window)
                .unwrap_or(null_mut());

            let registry_ptr: *mut WindowRegistry = self;
            let mut runtime = Box::new(Runtime {
                application: self.application,
                window_id: id,
                modal_parent: owner,
                renderer: Renderer::new(),
                window: null_mut(),
                focused: None,
                error: None,
                registry: registry_ptr,
                menu_commands: HashMap::new(),
                destroyed: false,
            });

            let runtime_ptr: *mut Runtime = &mut *runtime;
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    wide(WINDOW_CLASS_NAME).as_ptr(),
                    title.as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    size.width as i32,
                    size.height as i32,
                    owner,
                    null_mut(),
                    module_instance(),
                    runtime_ptr.cast(),
                )
            };
            if hwnd.is_null() {
                return Err(Error::windows_api("CreateWindowExW(top-level)"));
            }
            runtime.window = hwnd;

            // Register *before* any further native call. `CreateWindowExW`
            // above (and `ShowWindow` below) can synchronously deliver
            // messages — WM_SIZE in particular — to this window's own
            // `window_proc` before this function returns. Those handlers
            // dispatch through `Runtime::dispatch`, which calls back into
            // `sync()` on every dispatch (see its doc comment). If this
            // window were not yet in `self.runtimes` at that point, `sync()`
            // would see it as still missing from the native registry and
            // recurse into `create_window` for the same `WindowId` — which
            // creates *another* real native window, which can trigger the
            // same synchronous message, forever. This is not a hypothetical:
            // it is exactly what an earlier version of this function did.
            self.runtimes.insert(id, runtime);
            let runtime = self
                .runtimes
                .get_mut(&id)
                .expect("just inserted this window's runtime above");

            if let Some(menu) = &menu {
                let built = build_native_menu(menu)?;
                unsafe {
                    SetMenu(hwnd, built.handle);
                }
                runtime.menu_commands = built.commands;
            }

            let wake_target = hwnd as usize;
            application
                .scheduler_for(id)
                .expect("framework window must own a scheduler")
                .set_waker(std::sync::Arc::new(move || unsafe {
                    PostMessageW(wake_target as HWND, WM_FRAMEWORK_SCHEDULE, 0, 0);
                }));

            runtime.render()?;
            unsafe {
                ShowWindow(hwnd, SW_SHOW);
            }
            if !owner.is_null() {
                unsafe {
                    EnableWindow(owner, 0);
                }
            }

            Ok(())
        }
    }

    /// A native menu realized from a portable `MenuBar`, together with the
    /// command-id-to-`NodeId` table `window_proc` uses to translate a
    /// `WM_COMMAND` menu selection back into an `Event::MenuAction`.
    struct BuiltMenu {
        handle: HMENU,
        commands: HashMap<u16, NodeId>,
    }

    fn build_native_menu(menu: &MenuBar) -> Result<BuiltMenu, Error> {
        let handle = unsafe { CreateMenu() };
        if handle.is_null() {
            return Err(Error::windows_api("CreateMenu"));
        }
        let mut commands = HashMap::new();
        let mut next_command_id: u16 = 1;
        for item in menu.items() {
            append_menu_item(handle, item, &mut commands, &mut next_command_id)?;
        }
        Ok(BuiltMenu { handle, commands })
    }

    fn append_menu_item(
        parent: HMENU,
        item: &MenuItem,
        commands: &mut HashMap<u16, NodeId>,
        next_command_id: &mut u16,
    ) -> Result<(), Error> {
        if item.is_separator() {
            if unsafe { AppendMenuW(parent, MF_SEPARATOR, 0, null()) } == 0 {
                return Err(Error::windows_api("AppendMenuW(separator)"));
            }
            return Ok(());
        }

        let label = wide(item.label());
        if item.is_submenu() {
            let submenu = unsafe { CreatePopupMenu() };
            if submenu.is_null() {
                return Err(Error::windows_api("CreatePopupMenu"));
            }
            for child in item.children() {
                append_menu_item(submenu, child, commands, next_command_id)?;
            }
            let mut flags = MF_STRING | MF_POPUP;
            if !item.is_enabled() {
                flags |= MF_GRAYED;
            }
            if unsafe { AppendMenuW(parent, flags, submenu as usize, label.as_ptr()) } == 0 {
                return Err(Error::windows_api("AppendMenuW(submenu)"));
            }
        } else {
            let command_id = *next_command_id;
            *next_command_id = next_command_id.saturating_add(1);
            commands.insert(command_id, item.id());

            let mut flags = MF_STRING;
            if !item.is_enabled() {
                flags |= MF_GRAYED;
            }
            if item.is_checked() == Some(true) {
                flags |= MF_CHECKED;
            }
            if unsafe { AppendMenuW(parent, flags, command_id as usize, label.as_ptr()) } == 0 {
                return Err(Error::windows_api("AppendMenuW(item)"));
            }
        }
        Ok(())
    }

    pub(super) fn run_application(application: &mut Application) -> Result<(), Error> {
        let instance = module_instance();
        register_window_classes(instance)?;

        let mut registry = WindowRegistry {
            application: application as *mut Application,
            runtimes: HashMap::new(),
            creating: std::collections::HashSet::new(),
        };
        registry.sync()?;

        run_message_loop()?;

        for runtime in registry.runtimes.into_values() {
            if let Some(error) = runtime.error {
                return Err(error);
            }
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
            value if (0x30..=0x5A).contains(&value) => {
                KeyCode::Character(char::from_u32(value as u32).unwrap_or('?'))
            }
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
        if focus.is_null() {
            None
        } else {
            runtime.renderer.registry.id_for_hwnd(focus)
        }
    }

    fn focus_next(runtime: &mut Runtime, backwards: bool) {
        let focusable = runtime
            .renderer
            .snapshot
            .ordered_nodes()
            .into_iter()
            .filter(|node| {
                node.accessibility.focusable
                    && !node.disabled
                    && runtime.renderer.registry.get(node.id).is_some()
            })
            .collect::<Vec<_>>();
        if focusable.is_empty() {
            return;
        }

        let current = runtime
            .focused
            .and_then(|id| focusable.iter().position(|node| node.id == id));
        let next_index = match current {
            Some(index) if backwards => {
                if index == 0 {
                    focusable.len() - 1
                } else {
                    index - 1
                }
            }
            Some(index) => (index + 1) % focusable.len(),
            None if backwards => focusable.len() - 1,
            None => 0,
        };

        let next_id = focusable[next_index].id;
        if let Some(object) = runtime.renderer.registry.get(next_id) {
            unsafe {
                SetFocus(object.hwnd());
            }
        }
    }

    fn sync_focus(runtime: &mut Runtime) {
        let next = focused_node(runtime);
        if next == runtime.focused {
            return;
        }

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

    fn run_message_loop() -> Result<(), Error> {
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
                let runtime_ptr =
                    unsafe { GetWindowLongPtrW(message.hwnd, GWLP_USERDATA) } as *mut Runtime;
                if !runtime_ptr.is_null() {
                    unsafe { &mut *runtime_ptr }.pump_tasks()?;
                }
                continue;
            }

            let root = unsafe { GetAncestor(message.hwnd, GA_ROOT) };
            let runtime_ptr = unsafe { GetWindowLongPtrW(root, GWLP_USERDATA) } as *mut Runtime;
            if !runtime_ptr.is_null() {
                let runtime = unsafe { &mut *runtime_ptr };
                if message.message == WM_MOUSEWHEEL {
                    let mut point = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
                    unsafe {
                        GetCursorPos(&mut point);
                    }
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
                        focus_next(runtime, modifiers().shift);
                        sync_focus(runtime);
                        continue;
                    }
                    if let Err(error) = runtime.dispatch(Event::KeyDown {
                        target: focused_node(runtime),
                        key,
                        modifiers: modifiers(),
                    }) {
                        runtime.error = Some(error);
                        unsafe {
                            PostQuitMessage(1);
                        }
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
                                    unsafe {
                                        PostQuitMessage(1);
                                    }
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if !runtime_ptr.is_null() {
                sync_focus(unsafe { &mut *runtime_ptr });
            }
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
                let runtime_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Runtime;

                if !runtime_ptr.is_null() {
                    let runtime = unsafe { &mut *runtime_ptr };
                    if !control.is_null() {
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

                                let is_text_input =
                                    runtime.renderer.registry.get(id).is_some_and(|object| {
                                        matches!(object, NativeObject::TextInput(_))
                                    });
                                if is_text_input {
                                    let value = window_text(control);
                                    if let Err(error) =
                                        runtime.dispatch(Event::TextChanged { target: id, value })
                                    {
                                        runtime.error = Some(error);
                                        unsafe { PostQuitMessage(1) };
                                    }
                                }
                            }
                        }
                    } else if notification_code == 0 {
                        // A native menu command: no control window is
                        // associated with it (lParam is 0), unlike a
                        // control notification.
                        let command_id = (wparam & 0xffff) as u16;
                        if let Some(item) = runtime.menu_commands.get(&command_id).copied() {
                            if let Err(error) = runtime.dispatch(Event::MenuAction {
                                window: runtime.window_id,
                                item,
                            }) {
                                runtime.error = Some(error);
                                unsafe { PostQuitMessage(1) };
                            }
                        }
                    }
                }
                0
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
                let runtime_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Runtime;
                if runtime_ptr.is_null() {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
                let runtime = unsafe { &*runtime_ptr };
                let control = lparam as HWND;
                let hdc = wparam as HDC;
                let Some(id) = runtime.renderer.registry.id_for_hwnd(control) else {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                };
                let Some(style) = runtime.renderer.styles.get(&id) else {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                };
                unsafe {
                    SetTextColor(hdc, style.foreground);
                    SetBkColor(hdc, style.background);
                    SetBkMode(hdc, TRANSPARENT as i32);
                }
                style.background_brush as LRESULT
            }
            WM_SIZE => {
                let runtime_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Runtime;
                if !runtime_ptr.is_null() {
                    let runtime = unsafe { &mut *runtime_ptr };
                    let size = Size::new(lparam as u32 & 0xffff, (lparam as u32 >> 16) & 0xffff);
                    if let Err(error) = runtime.dispatch(Event::WindowResized {
                        window: runtime.window_id,
                        size,
                    }) {
                        runtime.error = Some(error);
                        unsafe {
                            PostQuitMessage(1);
                        }
                    } else {
                        let presentation = match wparam as u32 {
                            1 => framework_core::WindowPresentation::Minimized,
                            2 => framework_core::WindowPresentation::Maximized,
                            _ => framework_core::WindowPresentation::Normal,
                        };
                        if let Err(error) = runtime.dispatch(Event::WindowStateChanged {
                            window: runtime.window_id,
                            state: presentation,
                        }) {
                            runtime.error = Some(error);
                            unsafe {
                                PostQuitMessage(1);
                            }
                        }
                        runtime.relayout();
                    }
                }
                0
            }
            WM_MOVE => {
                let runtime_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Runtime;
                if !runtime_ptr.is_null() {
                    let runtime = unsafe { &mut *runtime_ptr };
                    let position = Point::new(
                        (lparam as u32 & 0xffff) as u16 as i16 as i32,
                        ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as i32,
                    );
                    if let Err(error) = runtime.dispatch(Event::WindowMoved {
                        window: runtime.window_id,
                        position,
                    }) {
                        runtime.error = Some(error);
                        unsafe {
                            PostQuitMessage(1);
                        }
                    }
                }
                0
            }
            WM_CLOSE => {
                let runtime_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Runtime;
                if !runtime_ptr.is_null() {
                    let runtime = unsafe { &mut *runtime_ptr };
                    if let Err(error) = runtime.dispatch(Event::WindowCloseRequested {
                        window: runtime.window_id,
                    }) {
                        runtime.error = Some(error);
                        unsafe {
                            PostQuitMessage(1);
                        }
                        return 0;
                    }
                    if runtime.window_id != WindowId::PRIMARY {
                        unsafe { &mut *runtime.application }.close_window(runtime.window_id);
                    }
                }
                unsafe { DestroyWindow(hwnd) };
                0
            }
            WM_DESTROY => {
                let runtime_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Runtime;
                if !runtime_ptr.is_null() {
                    let runtime = unsafe { &mut *runtime_ptr };
                    runtime.destroyed = true;
                    if !runtime.modal_parent.is_null() {
                        unsafe {
                            EnableWindow(runtime.modal_parent, 1);
                        }
                    }
                    if runtime.window_id == WindowId::PRIMARY {
                        unsafe { PostQuitMessage(0) };
                    }
                }
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
            WM_COMMAND | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
                // These are all sent to a control's *immediate* parent. A
                // container is frequently just one link in a chain of
                // nested containers, so forward up to the top-level window,
                // whose `window_proc` owns the `Runtime` these need.
                let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
                if !root.is_null() {
                    unsafe { SendMessageW(root, message, wparam, lparam) }
                } else {
                    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
                }
            }
            WM_ERASEBKGND => {
                // A container paints its own background directly: its
                // resolved background color is cached on its own
                // `GWLP_USERDATA` by `Renderer::apply_control_style`, since
                // (unlike the messages above) this one is sent to the
                // container itself, not to a parent that could look it up
                // through a `Runtime`.
                let colorref = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as u32;
                let brush = unsafe { CreateSolidBrush(colorref) };
                if brush.is_null() {
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
                let hdc = wparam as HDC;
                let mut client = RECT::default();
                unsafe {
                    GetClientRect(hwnd, &mut client);
                    FillRect(hdc, &client, brush);
                    DeleteObject(brush as HGDIOBJ);
                }
                1
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
