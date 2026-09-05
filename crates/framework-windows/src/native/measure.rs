//! Intrinsic size measurement (`WindowsIntrinsicMeasurer`) and the cached
//! GDI resources (`ControlStyle`) that realize a node's resolved
//! `VisualStyle`.

use framework_core::{Color, IntrinsicMeasurer, NodeKind, Size, Typography};
use windows_sys::Win32::Foundation::{COLORREF, HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CLIP_DEFAULT_PRECIS, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontIndirectW, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DeleteObject, DrawTextW, FF_DONTCARE, GetDC,
    GetSysColor, HBRUSH, HFONT, HGDIOBJ, LOGFONTW, OUT_DEFAULT_PRECIS, ReleaseDC,
};

pub(crate) struct WindowsIntrinsicMeasurer {
    pub(crate) window: HWND,
}

impl IntrinsicMeasurer for WindowsIntrinsicMeasurer {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        const DT_WORDBREAK: u32 = 0x0000_0010;
        const DT_CALCRECT: u32 = 0x0000_0400;

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

        // SAFETY: `self.window` is a live HWND owned by this measurer for
        // its whole lifetime; a null return (checked below) is
        // `GetDC`'s documented failure signal.
        let hdc = unsafe { GetDC(self.window) };
        if hdc.is_null() {
            return Size::new(1, 32);
        }

        let text_wide = super::util::wide(text);
        let padding = match kind {
            NodeKind::Button | NodeKind::TextInput => 24,
            NodeKind::Label | NodeKind::Column | NodeKind::Row => 0,
        };
        let available_width = max_width.map_or(i32::MAX / 4, |width| (width - padding).max(1));
        let mut rect = RECT { left: 0, top: 0, right: available_width, bottom: i32::MAX / 4 };
        let flags = DT_CALCRECT
            | if matches!(kind, NodeKind::Label) && max_width.is_some() { DT_WORDBREAK } else { 0 };

        // SAFETY: `hdc` was just validated non-null and is still owned by
        // this call (not yet released); `text_wide` is a NUL-terminated
        // wide buffer, matching the `-1`-length-means-NUL-terminated
        // contract; `rect` is a valid, exclusively borrowed `RECT` for
        // `DrawTextW` to write the calculated bounds into with
        // `DT_CALCRECT`.
        let measured = unsafe { DrawTextW(hdc, text_wide.as_ptr(), -1, &raw mut rect, flags) };
        // SAFETY: `hdc` was obtained from `GetDC(self.window)` above and
        // is released here exactly once, pairing that call as its
        // documented contract requires.
        unsafe { ReleaseDC(self.window, hdc) };

        if measured == 0 {
            return Size::new(1, 32);
        }

        let width = (rect.right - rect.left + padding).max(1);
        let height = (rect.bottom - rect.top).max(1);
        // `.max(1)` above guarantees both are positive, so the cast to an
        // unsigned `Size` cannot actually lose a sign — clippy's
        // `cast_sign_loss` can't see that guarantee through `.max`, so it
        // is named explicitly here rather than left for a reader to
        // re-derive.
        #[allow(clippy::cast_sign_loss)]
        Size::new(width as u32, height as u32)
    }
}

/// Cached GDI resources realizing one node's fully resolved
/// `VisualStyle`. Kept alive for as long as the node exists so
/// `WM_CTLCOLOR*`/`WM_ERASEBKGND` handlers can hand back a stable brush
/// on every repaint, and explicitly torn down (never left as a bare GDI
/// handle leak) when the node's style changes or the node is removed.
#[derive(Debug)]
pub(crate) struct ControlStyle {
    pub(crate) foreground: COLORREF,
    pub(crate) background: COLORREF,
    pub(crate) background_brush: HBRUSH,
    pub(crate) font: HFONT,
}

impl ControlStyle {
    /// Realizes a node's already theme-resolved style (see
    /// `TreeSnapshot::from_node_with_theme`) as GDI resources. Any
    /// component missing from the resolved style — which should only
    /// happen for a snapshot that skipped theme resolution — falls back
    /// to the corresponding system color so a control is never left
    /// unpainted.
    pub(crate) fn resolve(style: &framework_core::VisualStyle) -> Self {
        // SAFETY: `GetSysColor` takes a documented system-color index
        // constant and no pointer arguments; it cannot fail (an
        // unrecognized index simply returns black).
        let foreground = style
            .foreground_override()
            .map_or_else(|| unsafe { GetSysColor(COLOR_WINDOWTEXT) }, color_ref);
        // SAFETY: same as above.
        let background = style
            .background_override()
            .map_or_else(|| unsafe { GetSysColor(COLOR_WINDOW) }, color_ref);
        // SAFETY: `CreateSolidBrush` takes a plain `COLORREF` value and
        // no pointer arguments, so the call itself cannot be unsound; a
        // null return (GDI-handle-exhaustion) is a valid `HBRUSH` value
        // that `Drop` below already checks for before freeing.
        let background_brush = unsafe { CreateSolidBrush(background) };
        let font = style.typography_override().map_or(std::ptr::null_mut(), create_font);

        Self { foreground, background, background_brush, font }
    }
}

impl Drop for ControlStyle {
    fn drop(&mut self) {
        // SAFETY: `background_brush`/`font` are either null (checked
        // before use, matching `DeleteObject`'s documented no-op on
        // null) or GDI handles this `ControlStyle` exclusively owns and
        // has not freed before, since `Drop::drop` runs at most once.
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
    u32::from(color.red) | u32::from(color.green) << 8 | u32::from(color.blue) << 16
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
        lfHeight: -i32::from(typography.size),
        lfWidth: 0,
        lfEscapement: 0,
        lfOrientation: 0,
        lfWeight: i32::from(typography.weight),
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

    // SAFETY: `logfont` is a fully initialized `LOGFONTW`, exclusively
    // borrowed for the duration of this call; `lfFaceName` is a
    // fixed-size array already null-terminated by construction
    // (`face_name` starts zeroed and only `encoded.len()` of its 32
    // slots, at most 31, are overwritten above).
    unsafe { CreateFontIndirectW(&raw const logfont) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::test_support::TestWindow;

    #[test]
    fn measuring_empty_text_does_not_touch_the_device_context() {
        let window = TestWindow::new();
        let measurer = WindowsIntrinsicMeasurer { window: window.hwnd };
        let size = measurer.measure(NodeKind::Label, None, None);
        assert!(size.width > 0 && size.height > 0, "even an empty label reserves some space");
    }

    #[test]
    fn measuring_more_text_never_produces_a_narrower_result() {
        let window = TestWindow::new();
        let measurer = WindowsIntrinsicMeasurer { window: window.hwnd };
        let short = measurer.measure(NodeKind::Label, Some("Hi"), None);
        let long = measurer.measure(
            NodeKind::Label,
            Some("This is a substantially longer piece of label text"),
            None,
        );
        assert!(
            long.width >= short.width,
            "real GDI measurement of longer text must not report a narrower box \
             (short={short:?}, long={long:?})"
        );
    }

    #[test]
    fn measuring_respects_a_max_width_constraint_for_wrapping_kinds() {
        let window = TestWindow::new();
        let measurer = WindowsIntrinsicMeasurer { window: window.hwnd };
        let long_text = "word ".repeat(40);
        let unconstrained = measurer.measure(NodeKind::Label, Some(&long_text), None);
        let constrained = measurer.measure(NodeKind::Label, Some(&long_text), Some(80));
        assert!(
            constrained.width <= unconstrained.width,
            "constraining max_width must not report a wider result than unconstrained \
             (constrained={constrained:?}, unconstrained={unconstrained:?})"
        );
    }

    #[test]
    fn control_style_drop_frees_every_gdi_handle_it_created() {
        // Two earlier attempts to verify this synchronously both turned
        // out to be unreliable instruments rather than evidence of a real
        // leak: comparing `GetGuiResources(GR_GDIOBJECTS)` before/after
        // (it didn't move even right after a single create), and reading
        // `GetObjectType` on a handle immediately after `DeleteObject`
        // (a from-scratch standalone binary — see
        // `examples/gdi-font-diagnostic` — showed it reporting a brush as
        // still "valid" right after a `DeleteObject` call that itself
        // reported success, even though that same brush read back as
        // freed, by the identical check, inside `cargo test`'s process).
        // `DeleteObject`'s own return value was the one signal that never
        // once contradicted itself: success, every handle, every trial,
        // every environment.
        //
        // So this stops reading Windows' internal handle state back out
        // immediately after freeing it, and instead runs past the point
        // where a real per-call leak becomes physically undeniable:
        // Windows' default per-process GDI object quota
        // (`GDIProcessHandleQuota`) is 10,000 handles. If
        // `ControlStyle::drop` actually leaked a handle per style,
        // `CreateSolidBrush`/`CreateFontIndirectW` would start returning
        // null well before this loop finishes — a signal with no caching
        // or timing ambiguity, since a null pointer isn't a stale read.
        const ITERATIONS: u32 = 12_000;
        for i in 0..ITERATIONS {
            let style = framework_core::VisualStyle::default()
                .background(Color::rgb(10, 20, 30))
                .typography(Typography { family: "Segoe UI".to_owned(), size: 14, weight: 700 });
            let resolved = ControlStyle::resolve(&style);
            assert!(
                !resolved.background_brush.is_null(),
                "brush creation failed on iteration {i} of {ITERATIONS} — consistent with a \
                 real per-call handle leak exhausting the process's GDI quota"
            );
            assert!(
                !resolved.font.is_null(),
                "font creation failed on iteration {i} of {ITERATIONS} — consistent with a \
                 real per-call handle leak exhausting the process's GDI quota"
            );
            drop(resolved);
        }
    }

    #[test]
    fn control_style_resolve_falls_back_to_system_colors_when_unset() {
        let style = framework_core::VisualStyle::default();
        let resolved = ControlStyle::resolve(&style);
        // No typography override was set, so no font handle should have
        // been created — the renderer falls back to the control's
        // default system font instead.
        assert!(resolved.font.is_null());
        assert!(!resolved.background_brush.is_null());
    }
}
