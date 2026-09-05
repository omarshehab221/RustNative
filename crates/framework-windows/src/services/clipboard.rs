//! Native Win32 clipboard service.

#[cfg(windows)]
use super::run_blocking;
#[cfg(windows)]
use crate::ffi::GlobalFree;

/// Native Win32 clipboard service. Applications opt into it by injecting an
/// `Arc<WindowsClipboard>` into `framework_core::Services`.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsClipboard;

#[cfg(windows)]
#[async_trait::async_trait]
impl framework_core::ClipboardService for WindowsClipboard {
    async fn read_text(&self) -> Result<Option<String>, framework_core::ServiceError> {
        run_blocking(move || {
            use windows_sys::Win32::System::DataExchange::{
                CloseClipboard, GetClipboardData, OpenClipboard,
            };
            use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

            struct ClipboardGuard;
            impl Drop for ClipboardGuard {
                fn drop(&mut self) {
                    // SAFETY: this guard is only ever constructed
                    // immediately after `OpenClipboard` above succeeded on
                    // this same thread, and is not `Clone`, so exactly one
                    // matching `CloseClipboard` runs per successful open.
                    unsafe {
                        CloseClipboard();
                    }
                }
            }

            const CF_UNICODETEXT: u32 = 13;
            // SAFETY: a null owner window is a documented-valid argument to
            // `OpenClipboard` for reads not tied to a specific window.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
                return Err(framework_core::ServiceError::new("OpenClipboard failed"));
            }
            let _guard = ClipboardGuard;
            // SAFETY: `CF_UNICODETEXT` is a standard predefined clipboard
            // format; `GetClipboardData` may validly return null (checked
            // below) when no data of that format is present.
            let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
            if handle.is_null() {
                return Ok(None);
            }
            // SAFETY: `handle` is non-null and was just returned by
            // `GetClipboardData` while the clipboard is open on this
            // thread; `GlobalLock` on a valid `HGLOBAL` either returns a
            // valid pointer or null, which is checked immediately below.
            let value = unsafe { GlobalLock(handle) }.cast::<u16>();
            if value.is_null() {
                return Err(framework_core::ServiceError::new("GlobalLock clipboard data failed"));
            }
            let mut length = 0usize;
            // SAFETY: `CF_UNICODETEXT` data is contractually a
            // NUL-terminated UTF-16 buffer; `value` was just validated
            // non-null and remains locked (and therefore stable) for the
            // duration of this loop.
            while unsafe { *value.add(length) } != 0 {
                length += 1;
            }
            // SAFETY: `value` points at `length` contiguous, initialized
            // `u16`s (just walked above) inside memory still locked by the
            // `GlobalLock` call above.
            let text =
                String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value, length) });
            // SAFETY: `handle` is the same still-open handle locked above;
            // this pairs that lock exactly once.
            unsafe {
                GlobalUnlock(handle);
            }
            Ok(Some(text))
        })
        .await
    }

    async fn write_text(&self, text: String) -> Result<(), framework_core::ServiceError> {
        run_blocking(move || {
            use windows_sys::Win32::System::DataExchange::{
                CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
            };
            use windows_sys::Win32::System::Memory::{
                GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
            };

            struct ClipboardGuard;
            impl Drop for ClipboardGuard {
                fn drop(&mut self) {
                    // SAFETY: this guard is only constructed immediately
                    // after `OpenClipboard` above succeeded on this same
                    // thread, and is not `Clone`, so exactly one matching
                    // `CloseClipboard` runs per successful open.
                    unsafe {
                        CloseClipboard();
                    }
                }
            }

            const CF_UNICODETEXT: u32 = 13;
            let mut utf16 = text.encode_utf16().collect::<Vec<_>>();
            utf16.push(0);
            // SAFETY: a null owner window is a documented-valid argument to
            // `OpenClipboard` for writes not tied to a specific window.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
                return Err(framework_core::ServiceError::new("OpenClipboard failed"));
            }
            let _guard = ClipboardGuard;
            // SAFETY: the clipboard is open on this thread (checked above);
            // `EmptyClipboard` takes no pointer arguments.
            if unsafe { EmptyClipboard() } == 0 {
                return Err(framework_core::ServiceError::new("EmptyClipboard failed"));
            }
            // SAFETY: `GMEM_MOVEABLE` with a nonzero byte count is a
            // documented-valid `GlobalAlloc` call; a null return (checked
            // below) is the documented failure signal.
            let memory =
                unsafe { GlobalAlloc(GMEM_MOVEABLE, utf16.len() * std::mem::size_of::<u16>()) };
            if memory.is_null() {
                return Err(framework_core::ServiceError::new("GlobalAlloc clipboard data failed"));
            }
            // SAFETY: `memory` was just returned non-null by `GlobalAlloc`
            // above and is still owned by this function (not yet handed to
            // `SetClipboardData`).
            let destination = unsafe { GlobalLock(memory) }.cast::<u16>();
            if destination.is_null() {
                // SAFETY: `memory` is the same handle allocated above and
                // has not been freed or transferred yet, so freeing it here
                // on the lock-failure path is the correct, matching release.
                unsafe {
                    GlobalFree(memory);
                }
                return Err(framework_core::ServiceError::new("GlobalLock clipboard data failed"));
            }
            // SAFETY: `destination` is a non-null pointer to `utf16.len()`
            // `u16`s of freshly allocated, locked memory (validated above);
            // `utf16` is a distinct source allocation, so the ranges cannot
            // overlap. `GlobalUnlock` pairs the `GlobalLock` immediately
            // above on the same still-valid `memory` handle.
            unsafe {
                std::ptr::copy_nonoverlapping(utf16.as_ptr(), destination, utf16.len());
                GlobalUnlock(memory);
            }
            // SAFETY: `memory` is a valid `GMEM_MOVEABLE` handle populated
            // with `CF_UNICODETEXT`-format data above.
            if unsafe { SetClipboardData(CF_UNICODETEXT, memory) }.is_null() {
                // SAFETY: `SetClipboardData` failed, so ownership of
                // `memory` was never transferred to the system clipboard
                // (per its documented contract) and this function must free
                // it itself, exactly as on every other error path above.
                unsafe {
                    GlobalFree(memory);
                }
                return Err(framework_core::ServiceError::new("SetClipboardData failed"));
            }
            // On success, `SetClipboardData` has taken ownership of
            // `memory`; the system frees it, and it must NOT be freed here.
            Ok(())
        })
        .await
    }
}
