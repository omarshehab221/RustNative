//! Registers the top-level window class, runs the Win32 message loop, and
//! implements the top-level `WNDPROC`.

use framework_core::Event;
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, GetSysColorBrush, HDC, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, EN_CHANGE, GA_ROOT, GetAncestor, GetCursorPos, GetMessageW, MSG,
    PostQuitMessage, RegisterClassW, TranslateMessage, WM_CHAR, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MOVE, WM_NCCREATE, WM_SIZE, WNDCLASSW,
    WindowFromPoint,
};

use super::container::container_proc;
use super::input::{
    focus_next, focused_node, key_code, modifiers, set_hovered, set_pressed, sync_focus,
};
use super::registry::NativeObject;
use super::runtime::Runtime;
use super::user_data::RuntimeSlot;
use super::{
    CONTAINER_CLASS_NAME, EnableWindow, WINDOW_CLASS_NAME, WM_FRAMEWORK_SCHEDULE, WM_MOUSELEAVE,
};
use crate::Error;
use crate::native::util::window_text;

pub(crate) fn run_message_loop() -> Result<(), Error> {
    let mut message = MSG::default();

    loop {
        // SAFETY: `message` is a valid, exclusively borrowed `MSG` for
        // `GetMessageW` to write into; a null `hWnd` filter is the
        // documented way to retrieve messages for every window owned by
        // this thread. A `-1` return (checked below) is the documented
        // failure signal; `0` (also checked below) signals `WM_QUIT`.
        let result = unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) };

        if result == -1 {
            return Err(Error::windows_api("GetMessageW"));
        }

        if result == 0 {
            break;
        }

        if message.message == WM_FRAMEWORK_SCHEDULE {
            // `RuntimeSlot::get` returns a valid `*mut Runtime` (checked
            // non-null below) exactly when `message.hwnd` is a top-level
            // window whose `WM_NCCREATE` has already run and stored it
            // there (see `create_window_once`) — the only way this custom
            // message is ever posted is via the waker set up in that same
            // function, targeting that same hwnd.
            let runtime_ptr = RuntimeSlot::get(message.hwnd);
            if !runtime_ptr.is_null() {
                // SAFETY: `runtime_ptr` was just checked non-null and,
                // per the reasoning above, is a live `*mut Runtime`; the
                // message loop has exclusive access to every `Runtime`
                // between dispatches.
                unsafe { &mut *runtime_ptr }.pump_tasks()?;
            }
            continue;
        }

        // `GetAncestor` with `GA_ROOT` accepts any window handle and
        // returns null (handled by `RuntimeSlot::get`, itself always
        // safe on a null hwnd) if there is no such ancestor.
        // SAFETY: `message.hwnd` is the HWND Win32 just delivered this
        // message for.
        let root = unsafe { GetAncestor(message.hwnd, GA_ROOT) };
        let runtime_ptr = RuntimeSlot::get(root);
        if !runtime_ptr.is_null() {
            // SAFETY: `runtime_ptr` was just checked non-null and, per
            // the reasoning above, is a live `*mut Runtime`; the message
            // loop has exclusive access to every `Runtime` between
            // dispatches.
            let runtime = unsafe { &mut *runtime_ptr };
            match message.message {
                WM_MOUSEMOVE => {
                    let hovered = runtime.renderer.registry.id_for_hwnd(message.hwnd);
                    set_hovered(runtime, hovered);
                    if hovered.is_some() {
                        // `TRACKMOUSEEVENT` is a small, fixed-layout
                        // Win32 struct; its size can never approach
                        // `u32::MAX`, so this cannot actually truncate.
                        #[allow(clippy::cast_possible_truncation)]
                        let cb_size = std::mem::size_of::<TRACKMOUSEEVENT>() as u32;
                        let mut tracking = TRACKMOUSEEVENT {
                            cbSize: cb_size,
                            dwFlags: TME_LEAVE,
                            hwndTrack: message.hwnd,
                            dwHoverTime: 0,
                        };
                        // SAFETY: `tracking` is a fully initialized,
                        // exclusively borrowed `TRACKMOUSEEVENT`;
                        // `hwndTrack` is `message.hwnd`, the live HWND
                        // Win32 just delivered `WM_MOUSEMOVE` for.
                        unsafe {
                            TrackMouseEvent(&raw mut tracking);
                        }
                    }
                }
                WM_MOUSELEAVE => set_hovered(runtime, None),
                WM_LBUTTONDOWN => {
                    let pressed = runtime.renderer.registry.id_for_hwnd(message.hwnd);
                    set_pressed(runtime, pressed);
                }
                WM_LBUTTONUP => set_pressed(runtime, None),
                _ => {}
            }
            if message.message == WM_MOUSEWHEEL {
                let mut point = POINT { x: 0, y: 0 };
                // SAFETY: `point` is a valid, exclusively borrowed
                // `POINT` for `GetCursorPos` to write into.
                unsafe {
                    GetCursorPos(&raw mut point);
                }
                // SAFETY: `WindowFromPoint` takes a plain `POINT` value,
                // no pointer arguments; a null return is its documented
                // "no window at that point" signal, which
                // `scrollable_ancestor` already treats as "not found"
                // via its own null check on the same handle it walks.
                let hovered = unsafe { WindowFromPoint(point) };
                if let Some(id) = runtime.renderer.scrollable_ancestor(hovered) {
                    // The high word of `wParam` carries the wheel delta
                    // as a signed 16-bit value (`WHEEL_DELTA` units) per
                    // `WM_MOUSEWHEEL`'s documented layout; reinterpreting
                    // that exact `u16` as `i16` is the intentional,
                    // documented bit pattern this message defines, not a
                    // truncating or wrapping value change.
                    #[allow(clippy::cast_possible_wrap)]
                    let delta = super::util::hiword(message.wParam) as i16;
                    runtime.renderer.scroll_container(
                        id,
                        0,
                        -(i32::from(delta) / 3).clamp(-120, 120),
                    );
                    continue;
                }
            }

            if message.message == WM_KEYDOWN {
                // `wParam` for `WM_KEYDOWN` documents its virtual-key
                // code as fitting in the low byte; the truncation this
                // cast could in principle perform never actually happens
                // for any value Win32 delivers here.
                #[allow(clippy::cast_possible_truncation)]
                let key = key_code(message.wParam as u32);
                if key == framework_core::KeyCode::Tab {
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
                    // SAFETY: `PostQuitMessage` takes a plain exit-code
                    // integer and no pointer arguments.
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
                    // `wParam` for `WM_CHAR` documents its UTF-16 code
                    // unit as occupying the low word; this narrowing
                    // cannot lose information for any value Win32
                    // delivers here.
                    #[allow(clippy::cast_possible_truncation)]
                    let wparam_char = message.wParam as u32;
                    if let Some(character) = char::from_u32(wparam_char) {
                        if !character.is_control() {
                            if let Err(error) = runtime
                                .dispatch(Event::TextInput { target, text: character.to_string() })
                            {
                                runtime.error = Some(error);
                                // SAFETY: same as above — no pointer
                                // arguments.
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

        // SAFETY: `message` was just populated by `GetMessageW` above,
        // which succeeded (the `-1`/`0` failure and quit cases both
        // `return`/`break` before reaching here); both calls only read
        // it for the duration of this statement.
        unsafe {
            TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
        if !runtime_ptr.is_null() {
            // SAFETY: `runtime_ptr` was checked non-null and, per the
            // reasoning above, is a live `*mut Runtime`; the message
            // loop has exclusive access to every `Runtime` between
            // dispatches.
            sync_focus(unsafe { &mut *runtime_ptr });
        }
    }

    Ok(())
}

pub(crate) fn register_window_classes(instance: HINSTANCE) -> Result<(), Error> {
    register_window_class(instance, WINDOW_CLASS_NAME, window_proc)?;
    register_window_class(instance, CONTAINER_CLASS_NAME, container_proc)
}

fn register_window_class(
    instance: HINSTANCE,
    class_name: &str,
    window_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> Result<(), Error> {
    let class_name = super::util::wide(class_name);

    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        // SAFETY: `COLOR_WINDOW` is a documented standard system-color
        // index; `GetSysColorBrush` takes no pointer arguments and
        // returns a static system brush that must not be deleted.
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };

    // SAFETY: `class` is a fully initialized `WNDCLASSW`, exclusively
    // borrowed for the duration of this call; `class_name` (borrowed by
    // `class.lpszClassName`) is a NUL-terminated wide buffer that
    // outlives this call.
    let atom = unsafe { RegisterClassW(&raw const class) };
    if atom == 0 {
        const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;
        // SAFETY: `GetLastError` takes no arguments and is called
        // immediately after `RegisterClassW` reported failure, on the
        // same thread, before any other call could overwrite the
        // thread-local error code.
        let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if error != ERROR_CLASS_ALREADY_EXISTS {
            return Err(Error::WindowsApi { operation: "RegisterClassW", code: error });
        }
    }

    Ok(())
}

/// Wraps the body of a Win32 `WNDPROC` callback so a panic inside user
/// `Component` code cannot unwind across this `extern "system"` FFI
/// boundary — undefined behavior on stable Rust (standards audit P0.1).
/// A caught panic poisons the owning `Runtime` with a typed error and
/// posts `WM_QUIT`, so the message loop still exits, just cleanly and
/// through `run_application`'s ordinary error path instead of
/// potentially corrupting Win32's own call stack.
pub(crate) fn wndproc_boundary<F>(hwnd: HWND, f: F) -> LRESULT
where
    F: FnOnce() -> LRESULT + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_payload_message(&payload);
            // `RuntimeSlot::get` on every top-level window this crate
            // creates returns either null (WM_NCCREATE has not run yet)
            // or a live `*mut Runtime` set exactly once in WM_NCCREATE
            // and never reassigned to a dangling value for the window's
            // lifetime — see WM_NCCREATE below.
            let runtime_ptr = RuntimeSlot::get(hwnd);
            poison_runtime_and_quit(runtime_ptr, message);
            0
        }
    }
}

/// Shared tail of both `wndproc_boundary` variants: records the panic on
/// the owning `Runtime` (if resolved) and asks the message loop to exit.
/// Factored out once so the two callback-specific boundaries — which
/// necessarily resolve `runtime_ptr` differently (see
/// `container::container_wndproc_boundary`) — do not each re-derive this
/// SAFETY-critical step independently.
pub(crate) fn poison_runtime_and_quit(runtime_ptr: *mut Runtime, message: String) {
    if !runtime_ptr.is_null() {
        // SAFETY: the caller guarantees `runtime_ptr` is either null or
        // a live `*mut Runtime` per `RuntimeSlot`'s invariant (see
        // `user_data`'s module docs), and the Win32 message loop is
        // single-threaded and non-reentrant, so nothing else can be
        // concurrently mutating this `Runtime` right now.
        let runtime = unsafe { &mut *runtime_ptr };
        runtime.error = Some(Error::ComponentPanicked { message });
    }
    // SAFETY: `PostQuitMessage` takes no pointer arguments and is
    // always valid to call; it only queues `WM_QUIT` so the message
    // loop unwinds through its ordinary exit path.
    unsafe { PostQuitMessage(1) };
}

pub(crate) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "component panicked with a non-string payload".to_string()
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    wndproc_boundary(hwnd, move || window_proc_impl(hwnd, message, wparam, lparam))
}

fn window_proc_impl(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = lparam as *const CREATESTRUCTW;
            if create.is_null() {
                return 0;
            }

            // SAFETY: `create` was just checked non-null and, for a
            // window created through `create_window_once`, points at a
            // `CREATESTRUCTW` whose `lpCreateParams` Win32 forwards
            // unchanged from the `lpParam` given to `CreateWindowExW` —
            // here, `runtime_ptr.cast()`, a pointer into the
            // `Box<Runtime>` owned by `WindowRegistry::runtimes` for as
            // long as this window exists (see `WindowRegistry`'s type
            // doc comment: a `Runtime` outlives its own `WM_DESTROY`
            // and is only ever dropped after `run_application`'s
            // message loop returns).
            let runtime_ptr = unsafe { (*create).lpCreateParams }.cast::<Runtime>();
            // SAFETY: `hwnd` was just created by Win32 and is being
            // delivered its first message (`WM_NCCREATE` is always
            // first); storing this pointer here is exactly what
            // establishes the invariant every other `RuntimeSlot::get`
            // call in this file and in `wndproc_boundary` relies on.
            unsafe {
                RuntimeSlot::set(hwnd, runtime_ptr);
            }
            1
        }
        WM_COMMAND => {
            let notification_code = u32::from(super::util::hiword(wparam));
            let control = lparam as HWND;
            // Either null (before WM_NCCREATE above has run for this
            // hwnd) or the live `*mut Runtime` WM_NCCREATE stored, per
            // the invariant documented there.
            let runtime_ptr = RuntimeSlot::get(hwnd);

            if !runtime_ptr.is_null() {
                // SAFETY: non-null per the invariant above; the message
                // loop is single-threaded and non-reentrant.
                let runtime = unsafe { &mut *runtime_ptr };
                if !control.is_null() {
                    if let Some(id) = runtime.renderer.registry.id_for_hwnd(control) {
                        if notification_code == BN_CLICKED {
                            if let Err(error) = runtime.dispatch(Event::Click { target: id }) {
                                runtime.error = Some(error);
                                // SAFETY: `PostQuitMessage` takes no
                                // pointer arguments.
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
                                    // SAFETY: `PostQuitMessage` takes no
                                    // pointer arguments.
                                    unsafe { PostQuitMessage(1) };
                                }
                            }
                        }
                    }
                } else if notification_code == 0 {
                    // A native menu command: no control window is
                    // associated with it (lParam is 0), unlike a
                    // control notification.
                    let command_id = super::util::loword(wparam);
                    if let Some(item) = runtime.menu_commands.get(&command_id).copied() {
                        if let Err(error) =
                            runtime.dispatch(Event::MenuAction { window: runtime.window_id, item })
                        {
                            runtime.error = Some(error);
                            // SAFETY: `PostQuitMessage` takes no pointer
                            // arguments.
                            unsafe { PostQuitMessage(1) };
                        }
                    }
                }
            }
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
            // Read/dereference invariant — see WM_NCCREATE above.
            let runtime_ptr = RuntimeSlot::get(hwnd);
            if runtime_ptr.is_null() {
                // SAFETY: `hwnd`/`message`/`wparam`/`lparam` are exactly
                // what Win32 just delivered this callback with;
                // `DefWindowProcW`'s documented default handling is
                // valid for any window message.
                return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            }
            // SAFETY: non-null per the invariant above; the message
            // loop is single-threaded and non-reentrant.
            let runtime = unsafe { &*runtime_ptr };
            let control = lparam as HWND;
            let hdc = wparam as HDC;
            let Some(id) = runtime.renderer.registry.id_for_hwnd(control) else {
                // SAFETY: same as above.
                return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            };
            let Some(style) = runtime.renderer.styles.get(&id) else {
                // SAFETY: same as above.
                return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            };
            // SAFETY: `hdc` is the `HDC` Win32 passed via `wparam` for
            // this control-color message, valid for the duration of
            // this callback; `TRANSPARENT` is a documented mode
            // constant, not a pointer.
            unsafe {
                SetTextColor(hdc, style.foreground);
                SetBkColor(hdc, style.background);
                // `TRANSPARENT` is a small, fixed Win32 background-mode
                // constant (value `1`); it can never approach `i32::MAX`.
                #[allow(clippy::cast_possible_wrap)]
                let transparent_mode = TRANSPARENT as i32;
                SetBkMode(hdc, transparent_mode);
            }
            style.background_brush as LRESULT
        }
        WM_SIZE => {
            // Read/dereference invariant — see WM_NCCREATE above.
            let runtime_ptr = RuntimeSlot::get(hwnd);
            if !runtime_ptr.is_null() {
                // SAFETY: non-null per the invariant above; the message
                // loop is single-threaded and non-reentrant.
                let runtime = unsafe { &mut *runtime_ptr };
                let size = framework_core::Size::new(
                    u32::from(super::util::loword_signed(lparam)),
                    u32::from(super::util::hiword_signed(lparam)),
                );
                if let Err(error) =
                    runtime.dispatch(Event::WindowResized { window: runtime.window_id, size })
                {
                    runtime.error = Some(error);
                    // SAFETY: `PostQuitMessage` takes no pointer
                    // arguments.
                    unsafe {
                        PostQuitMessage(1);
                    }
                } else {
                    // `wParam` for `WM_SIZE` documents its resize-type
                    // flag (`SIZE_MINIMIZED` = 1, `SIZE_MAXIMIZED` = 2,
                    // ...) as a small constant that always fits in
                    // `u32`; a value large enough to truncate here would
                    // itself already indicate something has gone
                    // fundamentally wrong upstream, not a case this
                    // truncation could meaningfully guard against.
                    #[allow(clippy::cast_possible_truncation)]
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
                        // SAFETY: same as above.
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
            // Read/dereference invariant — see WM_NCCREATE above.
            let runtime_ptr = RuntimeSlot::get(hwnd);
            if !runtime_ptr.is_null() {
                // SAFETY: non-null per the invariant above; the message
                // loop is single-threaded and non-reentrant.
                let runtime = unsafe { &mut *runtime_ptr };
                // `WM_MOVE`'s `lParam` packs the window's signed x/y
                // position (negative on a multi-monitor setup with a
                // monitor to the left of/above the primary one) as two
                // 16-bit fields; reinterpreting each extracted `u16` as
                // `i16` is exactly that documented bit pattern, not a
                // truncating or wrapping value change.
                #[allow(clippy::cast_possible_wrap)]
                let position = framework_core::Point::new(
                    i32::from(super::util::loword_signed(lparam) as i16),
                    i32::from(super::util::hiword_signed(lparam) as i16),
                );
                if let Err(error) =
                    runtime.dispatch(Event::WindowMoved { window: runtime.window_id, position })
                {
                    runtime.error = Some(error);
                    // SAFETY: `PostQuitMessage` takes no pointer
                    // arguments.
                    unsafe {
                        PostQuitMessage(1);
                    }
                }
            }
            0
        }
        WM_CLOSE => {
            // Read/dereference invariant — see WM_NCCREATE above.
            let runtime_ptr = RuntimeSlot::get(hwnd);
            if !runtime_ptr.is_null() {
                // SAFETY: non-null per the invariant above; the message
                // loop is single-threaded and non-reentrant.
                let runtime = unsafe { &mut *runtime_ptr };
                if let Err(error) =
                    runtime.dispatch(Event::WindowCloseRequested { window: runtime.window_id })
                {
                    runtime.error = Some(error);
                    // SAFETY: `PostQuitMessage` takes no pointer
                    // arguments.
                    unsafe {
                        PostQuitMessage(1);
                    }
                    return 0;
                }
                if runtime.window_id != framework_core::WindowId::PRIMARY {
                    // SAFETY: `runtime.application` points to the
                    // mutable `Application` borrowed by
                    // `WindowsPlatform::run` and remains valid for this
                    // event loop (see `Runtime::render`); the event loop
                    // has exclusive access while it is running.
                    unsafe { &mut *runtime.application }.close_window(runtime.window_id);
                }
            }
            // SAFETY: `hwnd` is the HWND Win32 just invoked this
            // callback with, and is still live.
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            // Read/dereference invariant — see WM_NCCREATE above.
            let runtime_ptr = RuntimeSlot::get(hwnd);
            if !runtime_ptr.is_null() {
                // SAFETY: non-null per the invariant above; the message
                // loop is single-threaded and non-reentrant.
                let runtime = unsafe { &mut *runtime_ptr };
                runtime.destroyed = true;
                if !runtime.modal_parent.is_null() {
                    // SAFETY: `modal_parent` was just checked non-null
                    // and, per `create_window_once`, is a live HWND
                    // from `WindowRegistry::runtimes` that owns this
                    // window as a modal child.
                    unsafe {
                        EnableWindow(runtime.modal_parent, 1);
                    }
                }
                if runtime.window_id == framework_core::WindowId::PRIMARY {
                    // SAFETY: `PostQuitMessage` takes no pointer
                    // arguments.
                    unsafe { PostQuitMessage(0) };
                }
            }
            0
        }
        // SAFETY: `hwnd`/`message`/`wparam`/`lparam` are exactly what
        // Win32 just delivered this callback with; `DefWindowProcW`'s
        // documented default handling is valid for any window message.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
