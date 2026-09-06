//! A node's resolved visual style, realized as owned GDI resources.
//!
//! Two things live here, both concerned with the same question — "what GDI
//! objects does painting this node need, and who frees them?":
//!
//! - [`ControlStyle`], the per-node bundle of colors, brush, and font. It
//!   owns its GDI handles and frees them on drop, so a style change or a
//!   node removal cannot leak one.
//! - [`StyleCache`], the renderer's map from node to realized style, plus
//!   the transient interaction state (hover/press/focus) that feeds style
//!   resolution.
//! - [`background_brush_for`], a small process-wide brush cache keyed by
//!   color, used by container windows that paint their own background from
//!   a `WNDPROC` that cannot reach the renderer.

use std::collections::HashMap;

#[cfg(test)]
use framework_core::VisualStyle;
use framework_core::{
    Color, ControlState, NodeId, ResolvedStyle, StyleOverride, Theme, Typography,
};
use windows_sys::Win32::Foundation::{COLORREF, HWND};
use windows_sys::Win32::Graphics::Gdi::{
    CLIP_DEFAULT_PRECIS, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontIndirectW, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DeleteObject, FF_DONTCARE, GetSysColor,
    HBRUSH, HFONT, HGDIOBJ, LOGFONTW, OUT_DEFAULT_PRECIS,
};

use super::super::user_data::BackgroundColorSlot;

/// Cached GDI resources realizing one node's fully resolved `VisualStyle`.
///
/// Kept alive for as long as the node exists so `WM_CTLCOLOR*` handlers can
/// hand back a stable brush on every repaint, and explicitly torn down
/// (never left as a bare GDI handle leak) when the node's style changes or
/// the node is removed.
#[derive(Debug)]
pub(crate) struct ControlStyle {
    pub(crate) foreground: COLORREF,
    pub(crate) background: COLORREF,
    pub(crate) background_brush: HBRUSH,
    pub(crate) font: HFONT,
}

impl ControlStyle {
    /// Realizes a node's already theme-resolved style as GDI resources.
    ///
    /// Taking a [`ResolvedStyle`] rather than a bare `VisualStyle` is the
    /// point: holding one is proof that theme resolution happened, so this
    /// function cannot be handed a raw application override by mistake and
    /// paint a control with every themed property missing (standards audit
    /// P2.28). Any component still unset after resolution — which only
    /// happens for a snapshot that skipped theming entirely — falls back to
    /// the corresponding system color, so a control is never left unpainted.
    pub(crate) fn resolve(resolved: &ResolvedStyle) -> Self {
        let style = resolved.properties();
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

/// The renderer's realized styles, plus the transient interaction state
/// that selects which `ControlState` variant of a node's style applies.
///
/// Interaction state is deliberately *not* part of the declarative tree:
/// hovering a button must repaint it without rerendering the component that
/// produced it. Keeping both halves here means the "which style does this
/// node currently show?" question has exactly one owner.
#[derive(Debug, Default)]
pub(crate) struct StyleCache {
    realized: HashMap<NodeId, ControlStyle>,
    interaction: HashMap<NodeId, ControlState>,
    theme: Theme,
}

impl StyleCache {
    /// The realized style for `id`, if one has been applied.
    ///
    /// Read from the `WM_CTLCOLOR*` handler on every repaint, which is why
    /// the realized brush and colors are kept rather than re-resolved.
    pub(crate) fn get(&self, id: NodeId) -> Option<&ControlStyle> {
        self.realized.get(&id)
    }

    /// Replaces the theme every subsequent resolution is based on.
    pub(crate) fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Resolves and stores `id`'s style for the given node kind, override,
    /// and disabled flag, returning the realized resources.
    ///
    /// Storing the result also drops the *previous* `ControlStyle` for this
    /// node, which is what frees the GDI handles the last resolution
    /// created — style realization and style disposal are the same
    /// operation here, so neither can be forgotten.
    pub(crate) fn realize(
        &mut self,
        id: NodeId,
        kind: framework_core::NodeKind,
        style_override: &StyleOverride,
        disabled: bool,
    ) -> &ControlStyle {
        let state = if disabled {
            ControlState::Disabled
        } else {
            self.interaction.get(&id).copied().unwrap_or(ControlState::Normal)
        };
        let resolved = ControlStyle::resolve(&self.theme.resolve(kind, state, style_override));
        self.realized.insert(id, resolved);
        &self.realized[&id]
    }

    /// Records `id`'s live interaction state, returning whether it changed
    /// (and therefore whether the node needs restyling).
    pub(crate) fn set_interaction(&mut self, id: NodeId, state: ControlState) -> bool {
        let previous = self.interaction.get(&id).copied().unwrap_or(ControlState::Normal);
        if previous == state {
            return false;
        }
        if state == ControlState::Normal {
            self.interaction.remove(&id);
        } else {
            self.interaction.insert(id, state);
        }
        true
    }

    /// Drops everything realized for `id`, freeing its GDI handles.
    pub(crate) fn forget(&mut self, id: NodeId) {
        self.realized.remove(&id);
        self.interaction.remove(&id);
    }

    /// Drops interaction state for every node no longer in the tree.
    ///
    /// Realized styles are pruned by `forget` on the removal path instead,
    /// since a `TreeOp::Remove` names the node explicitly; interaction
    /// state can also be left behind by a node that vanished without one
    /// (a whole subtree replaced at once), so it is swept here.
    pub(crate) fn retain_interaction(&mut self, keep: impl Fn(NodeId) -> bool) {
        self.interaction.retain(|id, _| keep(*id));
    }
}

/// A brush for `hwnd`'s cached background color.
///
/// A window with nothing cached reads back the slot's zeroed initial value,
/// which is `RGB(0, 0, 0)` — so this always answers with *some* brush rather
/// than an `Option`. That is the right shape: the caller
/// (`container_proc`'s `WM_ERASEBKGND`) reaches this only for windows of
/// this crate's own container class, every one of which has its background
/// color written by `Renderer::apply_control_style` before it can be asked
/// to paint.
///
/// Container windows paint their own background from `container_proc`,
/// which — unlike the `WM_CTLCOLOR*` path — has no route back to the
/// `Renderer` and therefore no route to that node's [`ControlStyle`]. The
/// resolved color reaches it through the container's own `GWLP_USERDATA`
/// (see `native::user_data`), and this function turns that color into a
/// brush.
///
/// The brush comes from a small process-wide cache rather than being
/// created per call. `WM_ERASEBKGND` arrives on every repaint of every
/// container — many times a second during a drag-resize — and an earlier
/// revision created and destroyed a brush on each one; that is the
/// standards audit's P2.30 finding. A UI uses a handful of distinct
/// background colors, so the cache stays tiny, and its entries are
/// intentionally kept for the life of the process: they are shared by every
/// container painting that color, so there is no single owner that could
/// safely free one, and bounding it instead by *color count* means it
/// cannot grow with the number of nodes, repaints, or windows.
pub(crate) fn background_brush_for(hwnd: HWND) -> HBRUSH {
    cached_brush(BackgroundColorSlot::get(hwnd))
}

/// The process-wide color-to-brush cache backing [`background_brush_for`].
fn cached_brush(color: COLORREF) -> HBRUSH {
    use std::sync::Mutex;
    use std::sync::OnceLock;

    /// A cached `HBRUSH`, wrapped so the cache can be `Send`.
    ///
    /// GDI brush handles are not thread-affine (unlike an `HDC` or an
    /// `HWND`'s message queue): `FillRect` accepts a brush created on any
    /// thread. The wrapper exists because `HBRUSH` is a raw pointer type
    /// and so is `!Send` by default, not because there is thread-affinity
    /// to work around.
    struct SharedBrush(HBRUSH);

    // SAFETY: see the type's own doc comment — an `HBRUSH` is an opaque GDI
    // handle with no thread affinity, and every access here is additionally
    // serialized by the `Mutex` below.
    unsafe impl Send for SharedBrush {}

    static CACHE: OnceLock<Mutex<HashMap<COLORREF, SharedBrush>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    // A poisoned lock here means a previous caller panicked mid-insert. The
    // map itself is still structurally intact (a `HashMap` insert either
    // happened or did not), and the alternative — propagating the panic
    // from inside a `WNDPROC` — is strictly worse, so the guard is taken
    // either way.
    let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = cache.entry(color).or_insert_with(|| {
        // SAFETY: `CreateSolidBrush` takes a plain `COLORREF` value and no
        // pointer arguments. A null return means GDI handle exhaustion,
        // which callers treat as "no brush" via `FillRect` simply failing —
        // it is never dereferenced here.
        SharedBrush(unsafe { CreateSolidBrush(color) })
    });
    entry.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::test_support::TestWindow;

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
            let style =
                ResolvedStyle::new(
                    VisualStyle::default().background(Color::rgb(10, 20, 30)).typography(
                        Typography { family: "Segoe UI".to_owned(), size: 14, weight: 700 },
                    ),
                    ControlState::Normal,
                );
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
        let style = ResolvedStyle::new(VisualStyle::default(), ControlState::Normal);
        let resolved = ControlStyle::resolve(&style);
        // No typography override was set, so no font handle should have
        // been created — the renderer falls back to the control's
        // default system font instead.
        assert!(resolved.font.is_null());
        assert!(!resolved.background_brush.is_null());
    }

    #[test]
    fn the_background_brush_cache_returns_one_handle_per_color() {
        let first = TestWindow::new();
        let second = TestWindow::new();
        BackgroundColorSlot::set(first.hwnd, 0x00AA_BBCC);
        BackgroundColorSlot::set(second.hwnd, 0x00AA_BBCC);

        let one = background_brush_for(first.hwnd);
        let two = background_brush_for(second.hwnd);
        assert_eq!(
            one, two,
            "two windows sharing a background color must share one cached brush (P2.30)"
        );
    }

    #[test]
    fn the_background_brush_cache_survives_repeated_erases_without_exhausting_gdi() {
        // The pre-cache implementation created and destroyed a brush on
        // every `WM_ERASEBKGND`. This drives the same call path far past
        // the process GDI quota to prove the cached path allocates once
        // per color rather than once per erase.
        let window = TestWindow::new();
        BackgroundColorSlot::set(window.hwnd, 0x0001_0203);
        let first = background_brush_for(window.hwnd);
        for _ in 0..20_000 {
            let brush = background_brush_for(window.hwnd);
            assert_eq!(brush, first, "every erase of the same color reuses the one brush");
        }
    }

    #[test]
    fn interaction_state_reports_only_real_transitions() {
        let mut cache = StyleCache::default();
        let id = NodeId::from_key("button");
        assert!(!cache.set_interaction(id, ControlState::Normal), "Normal is the default");
        assert!(cache.set_interaction(id, ControlState::Hovered));
        assert!(!cache.set_interaction(id, ControlState::Hovered), "no change, no restyle");
        assert!(cache.set_interaction(id, ControlState::Normal), "back to default is a change");
    }
}
