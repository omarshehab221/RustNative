//! Intrinsic (content-driven) size measurement using real GDI text metrics.
//!
//! This is the Windows implementation of `framework_core`'s
//! [`IntrinsicMeasurer`] seam: the layout engine asks "how large does this
//! label's own content want to be?" and this answers with what the system
//! font actually measures, rather than the character-count heuristic
//! `DefaultIntrinsicMeasurer` falls back to.
//!
//! Measurement is the *only* thing here. The GDI resources that realize a
//! node's colors and font live in `super::rendering::styling`, which owns
//! their lifetime — see that subsystem's `mod.rs` for why measuring text and
//! owning brushes are separate responsibilities.

use framework_core::{IntrinsicMeasurer, NodeKind, Size};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{DrawTextW, GetDC, ReleaseDC};

use super::win32::best_effort;

/// Measures text against a real device context for one window, so a label's
/// intrinsic size reflects the font that will actually draw it.
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
        // `i32::MAX / 4` rather than `i32::MAX`: `DrawTextW` computes a
        // bounding box by adding to these bounds, so leaving three quarters
        // of the range as headroom keeps the arithmetic inside GDI from
        // overflowing on a pathologically long single line.
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
        let released = unsafe { ReleaseDC(self.window, hdc) } != 0;
        // Best effort *only* because there is nothing to report it to: a
        // genuinely failed release would leak a device context from the
        // system's small common cache, so it is asserted in debug builds
        // rather than passed over.
        best_effort(released, "ReleaseDC", "a leaked DC would exhaust the common cache");

        if measured == 0 {
            return Size::new(1, 32);
        }

        // Both `.max(1)` calls also bound the subtraction: `DrawTextW` with
        // `DT_CALCRECT` writes a normalized rectangle, so `right >= left`
        // and `bottom >= top`, and neither difference can exceed the
        // `i32::MAX / 4` bounds given above.
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
    fn measuring_a_pathologically_long_line_stays_within_the_coordinate_domain() {
        let window = TestWindow::new();
        let measurer = WindowsIntrinsicMeasurer { window: window.hwnd };
        let huge = "x".repeat(100_000);
        let size = measurer.measure(NodeKind::Label, Some(&huge), None);
        assert!(size.width > 0, "an enormous single line still measures to a usable width");
        assert!(size.height > 0);
    }
}
