//! Registers the top-level window class, runs the Win32 message loop, and
//! implements the top-level `WNDPROC`.

use framework_core::{Event, PanicAction, PanicReport};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, GetSysColorBrush, HDC, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, EN_CHANGE, GetCursorPos, GetMessageW, MSG, PostQuitMessage, RegisterClassW,
    TranslateMessage, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_MOVE, WM_NCCREATE, WM_QUIT, WM_SIZE, WNDCLASSW, WindowFromPoint,
};

use super::container::container_proc;
use super::context::{root_window, with_runtime};
use super::input::{
    focus_next, focused_node, key_code, modifiers, set_hovered, set_pressed, sync_focus,
};
use super::registry::NativeObject;
use super::runtime::Runtime;
use super::user_data::RuntimeSlot;
use super::win32::{best_effort, ignored_by_contract, informational};
use super::{
    CONTAINER_CLASS_NAME, EnableWindow, WINDOW_CLASS_NAME, WM_FRAMEWORK_SCHEDULE, WM_MOUSELEAVE,
};
use crate::Error;
use crate::error::{NativeContext, Win32Category};
use crate::native::util::window_text;

/// Whether the loop should keep going after a message, or stop because
/// `WM_QUIT` was posted.
///
/// `run_message_loop` learns this from `GetMessageW`'s return value, while
/// the test harness (`native::harness`) learns it from `PeekMessageW`
/// returning `WM_QUIT` as an ordinary message — which is exactly why
/// [`handle_message`] reports it rather than deciding for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopStep {
    /// Keep pumping.
    Continue,
    /// `WM_QUIT` was seen; unwind the loop.
    Quit,
}

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

        if handle_message(&message)? == LoopStep::Quit {
            break;
        }
    }

    Ok(())
}

/// Everything the message loop does with one message: framework-level
/// pre-processing, native dispatch, and the post-dispatch focus
/// reconciliation.
///
/// This is a function rather than the body of `run_message_loop` so that
/// the native integration harness pumps messages through the *same* code
/// the application does. A harness with its own parallel dispatch would
/// test itself rather than the backend — which is the trap the standards
/// audit's P1.17 finding warns about ("rather than pretending compile-time
/// tests cover native behavior").
pub(crate) fn handle_message(message: &MSG) -> Result<LoopStep, Error> {
    if message.message == WM_QUIT {
        return Ok(LoopStep::Quit);
    }

    if message.message == WM_FRAMEWORK_SCHEDULE {
        // The only thing that ever posts this custom message is the waker
        // `create_window_once` installs, targeting that same window's own
        // top-level hwnd — so this resolves the handle directly rather than
        // walking to an ancestor. A `None` result means the window has
        // since been destroyed and the wake is moot.
        with_runtime(message.hwnd, Runtime::pump_tasks).transpose()?;
        return Ok(LoopStep::Continue);
    }

    // Win32 addresses each message to whichever window it concerns —
    // frequently a native control or a nested container, not the top-level
    // window that owns the `Runtime`. Resolve the root once and reuse it
    // for both the pre-dispatch pass and the focus sync after dispatch.
    let root = root_window(message.hwnd);
    let flow = with_runtime(root, |runtime| pre_dispatch(runtime, message))
        .unwrap_or(MessageFlow::Dispatch);
    if flow == MessageFlow::Consumed {
        return Ok(LoopStep::Continue);
    }

    // SAFETY: `message` points at a `MSG` the caller just obtained from
    // `GetMessageW`/`PeekMessageW`; both calls only read it for the
    // duration of this statement.
    //
    // `TranslateMessage` reports whether the message *was* translated
    // (into a `WM_CHAR`), not whether it succeeded, and `DispatchMessageW`
    // returns the target `WNDPROC`'s own `LRESULT` — neither is a status
    // code this loop can act on.
    unsafe {
        ignored_by_contract(TranslateMessage(message));
        informational(DispatchMessageW(message));
    }

    // Native focus can have moved during dispatch (a click on a control, a
    // `SetFocus` from application code) without the framework hearing about
    // it, so reconcile after every message rather than only after the ones
    // that obviously change focus.
    with_runtime(root, sync_focus);
    Ok(LoopStep::Continue)
}

/// Whether a message this loop pre-processed still needs Win32's ordinary
/// `TranslateMessage`/`DispatchMessageW` treatment afterwards.
///
/// A few messages are fully consumed before dispatch — Tab is turned into
/// focus traversal rather than reaching a control, and a wheel event over a
/// scrollable container becomes a viewport transform — and forwarding those
/// on would let the native control act on them a second time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageFlow {
    /// Hand the message to Win32 as usual.
    Dispatch,
    /// The pre-dispatch step fully handled it; skip native dispatch.
    Consumed,
}

/// Framework-level handling that must happen *before* Win32 dispatches a
/// message to the target window's own procedure.
///
/// This exists as a separate pass, rather than as more arms inside
/// `window_proc`, because everything here concerns messages Win32 delivers
/// to a **child** window — a native control or a container — whose window
/// procedure is either the system's own (for `STATIC`/`BUTTON`/`EDIT`) or
/// `container_proc`. Neither can see the owning `Runtime` at the moment the
/// message arrives; the loop can, because it has just resolved the root.
fn pre_dispatch(runtime: &mut Runtime, message: &MSG) -> MessageFlow {
    match message.message {
        WM_MOUSEMOVE => {
            let hovered = runtime.renderer.registry.id_for_hwnd(message.hwnd);
            set_hovered(runtime, hovered);
            if hovered.is_some() {
                track_mouse_leave(message.hwnd);
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

    match message.message {
        WM_MOUSEWHEEL => wheel_scroll(runtime, message),
        WM_KEYDOWN => key_down(runtime, message),
        WM_CHAR => {
            character(runtime, message);
            MessageFlow::Dispatch
        }
        _ => MessageFlow::Dispatch,
    }
}

/// Asks Win32 to send one `WM_MOUSELEAVE` when the pointer next leaves
/// `hwnd`, which is the only way to learn that a control stopped being
/// hovered — there is no "mouse exited" message otherwise.
fn track_mouse_leave(hwnd: HWND) {
    // `TRACKMOUSEEVENT` is a small, fixed-layout Win32 struct; its size
    // can never approach `u32::MAX`, so this cannot actually truncate.
    #[allow(clippy::cast_possible_truncation)]
    let cb_size = std::mem::size_of::<TRACKMOUSEEVENT>() as u32;
    let mut tracking =
        TRACKMOUSEEVENT { cbSize: cb_size, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
    // SAFETY: `tracking` is a fully initialized, exclusively borrowed
    // `TRACKMOUSEEVENT`; `hwndTrack` is the live HWND Win32 just delivered
    // `WM_MOUSEMOVE` for.
    let tracked = unsafe { TrackMouseEvent(&raw mut tracking) } != 0;
    // Best effort: without the leave notification the control simply keeps
    // its hover styling until the pointer enters a different tracked
    // control, which is a cosmetic degradation, not an incorrect UI.
    best_effort(tracked, "TrackMouseEvent(TME_LEAVE)", "hover styling is cosmetic");
}

/// Translates a wheel event into a scroll of whichever scrollable container
/// is under the pointer, if any.
fn wheel_scroll(runtime: &mut Runtime, message: &MSG) -> MessageFlow {
    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: `point` is a valid, exclusively borrowed `POINT` for
    // `GetCursorPos` to write into.
    let located = unsafe { GetCursorPos(&raw mut point) } != 0;
    if !located {
        // The documented failure here is the calling thread lacking access
        // to the input desktop, in which case there is no pointer position
        // to scroll relative to at all.
        best_effort(located, "GetCursorPos", "there is no pointer position to scroll under");
        return MessageFlow::Dispatch;
    }
    // SAFETY: `WindowFromPoint` takes a plain `POINT` value, no pointer
    // arguments; a null return is its documented "no window at that point"
    // signal, which `scrollable_ancestor` already treats as "not found"
    // via its own null check on the same handle it walks.
    let hovered = unsafe { WindowFromPoint(point) };
    let Some(id) = runtime.renderer.scrollable_ancestor(hovered) else {
        return MessageFlow::Dispatch;
    };

    // The high word of `wParam` carries the wheel delta as a signed 16-bit
    // value (`WHEEL_DELTA` units) per `WM_MOUSEWHEEL`'s documented layout;
    // reinterpreting that exact `u16` as `i16` is the intentional,
    // documented bit pattern this message defines, not a truncating or
    // wrapping value change.
    #[allow(clippy::cast_possible_wrap)]
    let delta = super::util::hiword(message.wParam) as i16;
    runtime.renderer.scroll_container(id, 0, -(i32::from(delta) / 3).clamp(-120, 120));
    // A scroll is a viewport transform this framework owns; letting the
    // native control also act on the same wheel event would double-scroll.
    MessageFlow::Consumed
}

/// Turns a key press into either framework focus traversal (Tab) or a
/// `KeyDown` event for the focused component.
fn key_down(runtime: &mut Runtime, message: &MSG) -> MessageFlow {
    // `wParam` for `WM_KEYDOWN` documents its virtual-key code as fitting
    // in the low byte; the truncation this cast could in principle perform
    // never actually happens for any value Win32 delivers here.
    #[allow(clippy::cast_possible_truncation)]
    let key = key_code(message.wParam as u32);
    if key == framework_core::KeyCode::Tab {
        focus_next(runtime, modifiers().shift);
        sync_focus(runtime);
        // Tab is framework-level focus traversal; forwarding it would let
        // Win32's own dialog-manager-style handling move focus a second
        // time.
        return MessageFlow::Consumed;
    }
    runtime.dispatch_or_quit(Event::KeyDown {
        target: focused_node(runtime),
        key,
        modifiers: modifiers(),
    });
    MessageFlow::Dispatch
}

/// Delivers committed text to the focused component, for every focus target
/// except a native `EDIT`, which produces its own `EN_CHANGE` notification
/// and would otherwise report the same keystroke twice.
fn character(runtime: &mut Runtime, message: &MSG) {
    let target = focused_node(runtime);
    let native_text_input = target
        .and_then(|id| runtime.renderer.registry.get(id))
        .is_some_and(|object| matches!(object, NativeObject::TextInput(_)));
    if native_text_input {
        return;
    }
    // `wParam` for `WM_CHAR` documents its UTF-16 code unit as occupying
    // the low word; this narrowing cannot lose information for any value
    // Win32 delivers here.
    #[allow(clippy::cast_possible_truncation)]
    let wparam_char = message.wParam as u32;
    let Some(character) = char::from_u32(wparam_char) else {
        return;
    };
    if character.is_control() {
        return;
    }
    runtime.dispatch_or_quit(Event::TextInput { target, text: character.to_string() });
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
            return Err(Error::WindowsApi {
                operation: "RegisterClassW",
                code: error,
                category: Win32Category::of(error),
                // Class registration happens once, before any window
                // exists, so there is no window or node to name.
                context: NativeContext::none(),
            });
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
            let message = panic_payload_message(payload.as_ref());
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

/// Shared tail of both `wndproc_boundary` variants: records the caught
/// panic and applies the application's configured [`PanicPolicy`].
///
/// Factored out once so the two callback-specific boundaries — which
/// necessarily resolve `runtime_ptr` differently (see
/// `container::container_wndproc_boundary`) — do not each re-derive this
/// SAFETY-critical step independently.
///
/// A panic with no resolvable `Runtime` (a callback that panicked before
/// `WM_NCCREATE` published one) has no application to consult and nowhere
/// to record the error, so it always quits: continuing would mean running
/// on after a panic nobody can observe.
pub(crate) fn poison_runtime_and_quit(runtime_ptr: *mut Runtime, message: String) {
    if runtime_ptr.is_null() {
        quit_message_loop();
        return;
    }
    // SAFETY: the caller guarantees `runtime_ptr` is either null (returned
    // above) or a live `*mut Runtime` per `RuntimeSlot`'s invariant (see
    // `user_data`'s module docs), and the Win32 message loop is
    // single-threaded and non-reentrant, so nothing else can be
    // concurrently mutating this `Runtime` right now.
    let runtime = unsafe { &mut *runtime_ptr };
    let window = runtime.window_id;
    let report = PanicReport { message: message.clone(), window };
    let action =
        runtime.with_application(|application| application.handle_component_panic(&report));

    match action {
        PanicAction::Terminate => {
            let context =
                NativeContext::none().with_window(window).with_handle(runtime.window as usize);
            runtime.error = Some(Error::ComponentPanicked { message, context });
            quit_message_loop();
        }
        PanicAction::CloseWindow(target) => {
            // Deliberately routed through the same deferred path a normal
            // close uses (`Application::close_window`, picked up by
            // `WindowRegistry::sync`) rather than destroying the window
            // here. This code runs *inside* the panicking callback, with
            // that window's `Runtime` borrowed further up the stack;
            // tearing it down synchronously is the use-after-free
            // `WindowRegistry`'s own documentation describes.
            //
            // The error is not recorded: under this policy the panic is
            // handled, not fatal, and `run_application` returns
            // `Runtime::error` as the failure of `Platform::run`.
            runtime.with_application(|application| application.close_window(target));
            // A panic unwinds past the `sync_windows` at the tail of
            // `Runtime::dispatch`, so the close request just queued would
            // otherwise sit unapplied forever. Syncing here is what turns it
            // into a posted `WM_CLOSE`.
            if let Err(error) = runtime.sync_windows() {
                // The window could not be torn down, which leaves the
                // application in exactly the state `CloseWindow` was chosen
                // to avoid; fall back to the stronger policy rather than
                // carrying on with a window whose component just panicked.
                runtime.error = Some(error);
                quit_message_loop();
            }
        }
        // `PanicAction::Continue`, and any action a future version of
        // `framework-core` adds that this backend has not caught up with.
        // Continuing is the right default for an unrecognized action: the
        // panic was already caught before it could unwind across the FFI
        // boundary, which is the part that had to happen, and inventing a
        // termination this backend was not asked for would be worse than
        // doing nothing.
        _ => {}
    }
}

/// Asks the message loop to exit through its ordinary path.
fn quit_message_loop() {
    // SAFETY: `PostQuitMessage` takes no pointer arguments and is always
    // valid to call; it only queues `WM_QUIT` so the message loop unwinds
    // through its ordinary exit path.
    unsafe { PostQuitMessage(1) };
}

/// Extracts a human-readable message from a caught panic's payload.
///
/// The parameter is the *unboxed* payload, and both callers must pass
/// `payload.as_ref()` rather than `&payload`. That is not a style
/// preference: `Box<dyn Any + Send>` is itself `Sized`, `Send`, and
/// `'static`, so it satisfies `Any` too — `&payload` coerces the **box** to
/// `&dyn Any` instead of dereferencing to the value inside it, and every
/// `downcast_ref` then misses. This code had exactly that bug, and it was
/// invisible until the native integration tests provoked a real component
/// panic and read the message back: every caught panic reported
/// "component panicked with a non-string payload", discarding the one piece
/// of diagnostic information a caught panic carries.
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
    // Delegating to the default window procedure, which every arm below
    // that declines to handle a message falls back to.
    //
    // SAFETY: `hwnd`/`message`/`wparam`/`lparam` are exactly what Win32
    // just delivered this callback with; `DefWindowProcW`'s documented
    // default handling is valid for any window message.
    let default = || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };

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
            // establishes the invariant every `native::context`
            // resolution in this crate relies on.
            unsafe {
                RuntimeSlot::set(hwnd, runtime_ptr);
            }
            1
        }
        WM_COMMAND => {
            with_runtime(hwnd, |runtime| {
                let notification_code = u32::from(super::util::hiword(wparam));
                let control = lparam as HWND;
                if control.is_null() {
                    // A native menu command: no control window is
                    // associated with it (`lParam` is 0), unlike a control
                    // notification, and its notification code is 0.
                    if notification_code == 0 {
                        menu_command(runtime, super::util::loword(wparam));
                    }
                } else if let Some(id) = runtime.renderer.registry.id_for_hwnd(control) {
                    control_notification(runtime, id, notification_code, control);
                }
            });
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
            with_runtime(hwnd, |runtime| control_color(runtime, wparam, lparam))
                .flatten()
                .unwrap_or_else(default)
        }
        WM_SIZE => {
            with_runtime(hwnd, |runtime| {
                let size = framework_core::Size::new(
                    u32::from(super::util::loword_signed(lparam)),
                    u32::from(super::util::hiword_signed(lparam)),
                );
                let window = runtime.window_id;
                if !runtime.dispatch_or_quit(Event::WindowResized { window, size }) {
                    return;
                }
                // `wParam` for `WM_SIZE` documents its resize-type flag
                // (`SIZE_MINIMIZED` = 1, `SIZE_MAXIMIZED` = 2, ...) as a
                // small constant that always fits in `u32`; a value large
                // enough to truncate here would itself already indicate
                // something has gone fundamentally wrong upstream, not a
                // case this truncation could meaningfully guard against.
                #[allow(clippy::cast_possible_truncation)]
                let state = match wparam as u32 {
                    1 => framework_core::WindowPresentation::Minimized,
                    2 => framework_core::WindowPresentation::Maximized,
                    _ => framework_core::WindowPresentation::Normal,
                };
                if runtime.dispatch_or_quit(Event::WindowStateChanged { window, state }) {
                    runtime.relayout();
                }
            });
            0
        }
        WM_MOVE => {
            with_runtime(hwnd, |runtime| {
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
                let window = runtime.window_id;
                runtime.dispatch_or_quit(Event::WindowMoved { window, position });
            });
            0
        }
        WM_CLOSE => {
            let destroy = with_runtime(hwnd, |runtime| {
                let window = runtime.window_id;
                if !runtime.dispatch_or_quit(Event::WindowCloseRequested { window }) {
                    // The dispatch already recorded the error and posted
                    // `WM_QUIT`; tearing the window down on top of that
                    // would run destruction during an error unwind.
                    return false;
                }
                // The primary window's close is the application's own exit
                // path, handled by `WM_DESTROY` posting `WM_QUIT`; any
                // other window must additionally be removed from the
                // application's window set so `WindowRegistry::sync` does
                // not immediately recreate it.
                if window != framework_core::WindowId::PRIMARY {
                    runtime.with_application(|application| application.close_window(window));
                }
                true
            });
            // A window with no runtime published yet is still a real window
            // that asked to close, so honor it.
            if destroy.unwrap_or(true) {
                // SAFETY: `hwnd` is the HWND Win32 just invoked this
                // callback with, and is still live.
                //
                // Best effort: the documented failure is `hwnd` not being
                // a window this thread owns, which cannot be true inside
                // its own `WNDPROC`, and there is nothing left to report a
                // failure to on a teardown path.
                let destroyed = unsafe { DestroyWindow(hwnd) } != 0;
                best_effort(destroyed, "DestroyWindow", "this is the window's own teardown path");
            }
            0
        }
        WM_DESTROY => {
            with_runtime(hwnd, |runtime| {
                runtime.destroyed = true;
                // Stop advertising this window as a dialog-owner candidate
                // before it actually stops existing — see
                // `native::window_handles`'s module doc comment.
                super::window_handles::clear(runtime.window_id);
                if !runtime.modal_parent.is_null() {
                    // SAFETY: `modal_parent` was just checked non-null
                    // and, per `create_window_once`, is a live HWND from
                    // `WindowRegistry::runtimes` that owns this window as
                    // a modal child.
                    //
                    // `EnableWindow` reports the window's *previous*
                    // disabled state, not whether the call worked.
                    ignored_by_contract(unsafe { EnableWindow(runtime.modal_parent, 1) });
                }
                if runtime.window_id == framework_core::WindowId::PRIMARY {
                    // SAFETY: `PostQuitMessage` takes no pointer
                    // arguments.
                    unsafe { PostQuitMessage(0) };
                }
            });
            0
        }
        _ => default(),
    }
}

/// Routes a native menu selection back to the component tree.
///
/// A command id with no entry in `menu_commands` is ignored rather than
/// treated as an error: Windows itself sends `WM_COMMAND` for system menu
/// items (Close, Minimize, ...) this crate never registered.
fn menu_command(runtime: &mut Runtime, command_id: u16) {
    let Some(item) = runtime.menu_commands.get(&command_id).copied() else {
        return;
    };
    let window = runtime.window_id;
    runtime.dispatch_or_quit(Event::MenuAction { window, item });
}

/// Routes a native control's notification (a button click, an edit
/// control's text change) back to the component that owns the control.
fn control_notification(
    runtime: &mut Runtime,
    id: framework_core::NodeId,
    notification_code: u32,
    control: HWND,
) {
    if notification_code == BN_CLICKED {
        runtime.dispatch_or_quit(Event::Click { target: id });
        return;
    }
    if notification_code != EN_CHANGE {
        return;
    }
    // A text change this renderer itself just wrote via `SetWindowTextW`,
    // not one the person typed — echoing it back as an event would loop.
    if runtime.renderer.suppress_text_change.remove(&id) {
        return;
    }
    let is_text_input = runtime
        .renderer
        .registry
        .get(id)
        .is_some_and(|object| matches!(object, NativeObject::TextInput(_)));
    if !is_text_input {
        return;
    }
    let value = window_text(control);
    runtime.dispatch_or_quit(Event::TextChanged { target: id, value });
}

/// Answers a `WM_CTLCOLOR*` message with the resolved foreground/background
/// of the control it concerns, returning `None` when this crate has no
/// realized style for that control and Win32's default painting should
/// stand.
fn control_color(runtime: &Runtime, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let control = lparam as HWND;
    let hdc = wparam as HDC;
    let id = runtime.renderer.registry.id_for_hwnd(control)?;
    let style = runtime.renderer.styles.get(id)?;
    // SAFETY: `hdc` is the `HDC` Win32 passed via `wparam` for this
    // control-color message, valid for the duration of this callback;
    // `TRANSPARENT` is a documented mode constant, not a pointer.
    //
    // All three setters return the *previous* color/mode rather than a
    // status code.
    unsafe {
        informational(SetTextColor(hdc, style.foreground));
        informational(SetBkColor(hdc, style.background));
        // `TRANSPARENT` is a small, fixed Win32 background-mode constant
        // (value `1`); it can never approach `i32::MAX`.
        #[allow(clippy::cast_possible_wrap)]
        let transparent_mode = TRANSPARENT as i32;
        informational(SetBkMode(hdc, transparent_mode));
    }
    Some(style.background_brush as LRESULT)
}
