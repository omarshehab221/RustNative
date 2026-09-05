//! The window procedure for `Column`/`Row` container windows: forwards
//! control notifications up to the owning top-level window and paints its
//! own cached background color.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HDC, HGDIOBJ};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GA_ROOT, GetAncestor, GetClientRect, SendMessageW, WM_COMMAND, WM_CTLCOLORBTN,
    WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_ERASEBKGND,
};

use super::message_loop::{panic_payload_message, poison_runtime_and_quit};
use super::user_data::{BackgroundColorSlot, RuntimeSlot};

pub(crate) unsafe extern "system" fn container_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    container_wndproc_boundary(hwnd, move || container_proc_impl(hwnd, message, wparam, lparam))
}

/// Same purpose as `message_loop::wndproc_boundary`, for container windows.
/// A container's own `GWLP_USERDATA` holds a cached background color (see
/// `WM_ERASEBKGND` in `container_proc_impl`), not a `*mut Runtime` — see
/// `user_data`'s module docs — so the owning `Runtime` is resolved through
/// the top-level ancestor window instead, via [`RuntimeSlot::get`] — the
/// same lookup `WM_COMMAND`/`WM_CTLCOLOR*` forwarding already relies on
/// below.
fn container_wndproc_boundary<F>(hwnd: HWND, f: F) -> LRESULT
where
    F: FnOnce() -> LRESULT + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_payload_message(&payload);
            // SAFETY: `hwnd` is the HWND Win32 just invoked this
            // callback with; `GetAncestor` with `GA_ROOT` accepts any
            // window handle and returns null (checked below) if there
            // is no such ancestor.
            let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
            let runtime_ptr =
                if root.is_null() { std::ptr::null_mut() } else { RuntimeSlot::get(root) };
            poison_runtime_and_quit(runtime_ptr, message);
            0
        }
    }
}

fn container_proc_impl(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_COMMAND | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
            // These are all sent to a control's *immediate* parent. A
            // container is frequently just one link in a chain of
            // nested containers, so forward up to the top-level window,
            // whose `window_proc` owns the `Runtime` these need.
            // SAFETY: `hwnd` is the HWND Win32 just invoked this
            // callback with; `GetAncestor` with `GA_ROOT` accepts any
            // window handle and returns null (checked below) if there
            // is no such ancestor.
            let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
            if root.is_null() {
                // SAFETY: `hwnd`/`message`/`wparam`/`lparam` are exactly
                // what Win32 just delivered this callback with;
                // `DefWindowProcW`'s documented default handling is
                // valid for any window message.
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            } else {
                // SAFETY: `root` was just checked non-null and, per
                // `GetAncestor`'s contract, is a live top-level HWND;
                // `message`/`wparam`/`lparam` are forwarded unchanged
                // from what Win32 delivered to this callback, which
                // `window_proc`'s handlers for these same message
                // values already expect in that shape.
                unsafe { SendMessageW(root, message, wparam, lparam) }
            }
        }
        WM_ERASEBKGND => {
            // A container paints its own background directly: its
            // resolved background color is cached on its own
            // `GWLP_USERDATA` by `Renderer::apply_control_style`, since
            // (unlike the messages above) this one is sent to the
            // container itself, not to a parent that could look it up
            // through a `Runtime`.
            let colorref = BackgroundColorSlot::get(hwnd);
            // SAFETY: `CreateSolidBrush` takes a plain `COLORREF` value
            // and no pointer arguments; a null return (checked below) is
            // its documented failure signal.
            let brush = unsafe { CreateSolidBrush(colorref) };
            if brush.is_null() {
                // SAFETY: `hwnd`/`message`/`wparam`/`lparam` are exactly
                // what Win32 just delivered this callback with;
                // `DefWindowProcW`'s documented default handling is
                // valid for any window message.
                return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            }
            let hdc = wparam as HDC;
            let mut client = RECT::default();
            // SAFETY: `hwnd` is a live HWND owned by this callback;
            // `client` is a valid, exclusively borrowed `RECT` for
            // `GetClientRect` to write into; `hdc` is the `HDC` Win32
            // passed via `wparam` for this erase-background message,
            // valid for the duration of this callback; `brush` was just
            // checked non-null and is freed exactly once, after its
            // last use, in this same block.
            unsafe {
                GetClientRect(hwnd, &raw mut client);
                FillRect(hdc, &raw const client, brush);
                DeleteObject(brush as HGDIOBJ);
            }
            1
        }
        // SAFETY: `hwnd`/`message`/`wparam`/`lparam` are exactly what
        // Win32 just delivered this callback with; `DefWindowProcW`'s
        // documented default handling is valid for any window message.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
