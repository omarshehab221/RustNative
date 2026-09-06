//! Creating, updating, and destroying the native Win32 window for one node.
//!
//! Everything in this module answers a single question: *which Win32 window
//! class and styles realize this `NodeKind`, and how is its text kept in
//! sync?* It deliberately knows nothing about visual styling, accessibility,
//! layout, or scrolling — the standards audit's P1.21 finding calls out
//! exactly this mixing ("changing layout should not modify HWND creation";
//! "accessibility semantics should not be implemented inside generic control
//! creation"), and keeping those out is what makes adding a control kind a
//! change to this file alone.
//!
//! # Failure and ownership
//!
//! Every creation path here has the same shape: create the window, check
//! for the documented null-handle failure, then hand ownership to the
//! registry — and if *that* fails, destroy the window before returning,
//! because at that point this function is still its only owner. A container
//! creates two windows and unwinds both. That is the audit's "resource
//! ownership belongs to RAII resource types, not to high-level renderer
//! methods" rule at the one boundary where a raw handle unavoidably exists:
//! between `CreateWindowExW` returning and the registry adopting it.

use std::ptr::{null, null_mut};

use framework_core::{NodeKind, TreeNode};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::SystemServices::{SS_LEFT, SS_NOPREFIX};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_PUSHBUTTON, CreateWindowExW, DestroyWindow, ES_AUTOHSCROLL, ES_LEFT, SetWindowTextW,
    WS_BORDER, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_CONTROLPARENT, WS_VISIBLE,
};

use super::super::CONTAINER_CLASS_NAME;
use super::super::registry::{NativeObject, NativeObjectRegistry};
use super::super::util::{module_instance, wide, window_text};
use super::super::win32::{best_effort, must_succeed};
use crate::Error;

/// Creates the native object realizing `node` and registers it under
/// `node.id`.
///
/// Controls are created *without* `WS_TABSTOP`, even for kinds that are
/// conventionally keyboard-focusable. Keyboard participation is a property
/// of the node's portable accessibility model, not of its control class, so
/// it is applied immediately afterwards by
/// [`super::accessibility::AccessibilityBridge`] from the node's actual
/// declared focusability. An earlier revision baked `WS_TABSTOP` into the
/// creation styles for buttons and text inputs regardless of what the node
/// declared, which the standards audit's P1.16 finding names directly
/// ("text inputs are created with `WS_TABSTOP` regardless of the portable
/// focusability flag").
pub(crate) fn create(
    registry: &mut NativeObjectRegistry,
    node: &TreeNode,
    parent: HWND,
) -> Result<(), Error> {
    match node.kind {
        NodeKind::Column | NodeKind::Row => create_container(registry, node, parent),
        NodeKind::Label => create_simple(
            registry,
            node,
            parent,
            "STATIC",
            SS_LEFT | SS_NOPREFIX,
            NativeObject::Label,
        ),
        NodeKind::Button => create_simple(
            registry,
            node,
            parent,
            "BUTTON",
            BS_PUSHBUTTON as u32,
            NativeObject::Button,
        ),
        NodeKind::TextInput => create_simple(
            registry,
            node,
            parent,
            "EDIT",
            WS_BORDER | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            NativeObject::TextInput,
        ),
    }
}

/// Whether the native object currently registered for `node` is still the
/// right *kind* of object for it.
///
/// A node can keep its identity across a rerender while changing kind (a
/// `Label` becoming a `Button` under the same key). Win32 has no way to
/// change a live window's class, so the only correct response is to tear
/// the old window down and build the new one.
pub(crate) fn needs_replacement(registry: &NativeObjectRegistry, node: &TreeNode) -> bool {
    !matches!(
        (node.kind, registry.get(node.id)),
        (NodeKind::Column | NodeKind::Row, Some(NativeObject::Container { .. }))
            | (NodeKind::Label, Some(NativeObject::Label(_)))
            | (NodeKind::Button, Some(NativeObject::Button(_)))
            | (NodeKind::TextInput, Some(NativeObject::TextInput(_)))
    )
}

/// Writes `node`'s text to its native control, if that control displays
/// text at all.
///
/// Returns whether the caller should suppress the resulting change
/// notification: a native `EDIT` raises `EN_CHANGE` for text *this* code
/// wrote, indistinguishably from text the person typed, and echoing that
/// back as a `TextChanged` event would loop the component against itself.
/// The text is also compared before writing, so an unchanged value neither
/// disturbs the caret nor raises a notification in the first place.
pub(crate) fn update_text(registry: &NativeObjectRegistry, node: &TreeNode) -> Result<bool, Error> {
    let Some(object) = registry.get(node.id) else {
        return Ok(false);
    };
    let hwnd = object.hwnd();

    match node.kind {
        NodeKind::Column | NodeKind::Row => Ok(false),
        NodeKind::Label | NodeKind::Button => {
            let text = wide(node.text.as_deref().unwrap_or_default());
            // SAFETY: `hwnd` is a live HWND owned by the registry entry
            // matched above; `text` is a NUL-terminated wide buffer kept
            // alive for the duration of this synchronous call.
            let written = unsafe { SetWindowTextW(hwnd, text.as_ptr()) } != 0;
            // Best effort: a static or button that failed to update its
            // caption shows stale text until the next update, which is a
            // visual defect rather than a state inconsistency — unlike the
            // `EDIT` case below, nothing else keys off this text.
            best_effort(written, "SetWindowTextW(caption)", "the control shows stale text");
            Ok(false)
        }
        NodeKind::TextInput => {
            let Some(value) = node.text.as_deref() else {
                return Ok(false);
            };
            if window_text(hwnd) == value {
                return Ok(false);
            }
            let text = wide(value);
            // SAFETY: as above.
            let written = unsafe { SetWindowTextW(hwnd, text.as_ptr()) } != 0;
            // Must succeed, unlike a caption: the component's state now says
            // the field holds `value`, and a field that silently kept its old
            // content would leave the two permanently disagreeing with no
            // event to reconcile them.
            must_succeed(written, "SetWindowTextW(EDIT)")?;
            Ok(true)
        }
    }
}

/// Creates one of the predefined system control classes.
fn create_simple(
    registry: &mut NativeObjectRegistry,
    node: &TreeNode,
    parent: HWND,
    class_name: &'static str,
    extra_style: u32,
    wrap: fn(HWND) -> NativeObject,
) -> Result<(), Error> {
    let text = wide(node.text.as_deref().unwrap_or_default());
    let class = wide(class_name);
    // SAFETY: `class`/`text` are NUL-terminated wide buffers naming a
    // predefined system window class and the initial window text; `parent`
    // is a live HWND owned by the renderer (or the top-level window passed
    // down from `render`); a null `lpParam` is a documented valid value
    // these classes' default window procedures do not read; a null return
    // (checked below) is `CreateWindowExW`'s documented failure signal.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | extra_style,
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
    adopt(registry, node, hwnd, wrap, class_name)
}

/// Creates the viewport/content window pair backing a scrollable container.
fn create_container(
    registry: &mut NativeObjectRegistry,
    node: &TreeNode,
    parent: HWND,
) -> Result<(), Error> {
    let class = wide(CONTAINER_CLASS_NAME);
    // SAFETY: `class` is a NUL-terminated wide buffer naming the window
    // class `register_window_classes` registers before any window is
    // created; `parent` is a live HWND owned by the renderer (or the
    // top-level window passed down from `render`); a null `lpParam` is a
    // documented valid value the resulting `WM_NCCREATE`/`WM_CREATE`
    // handlers here do not read; a null return (checked below) is
    // `CreateWindowExW`'s documented failure signal.
    //
    // `WS_EX_CONTROLPARENT` marks the viewport as a container Win32's own
    // dialog-style keyboard navigation should recurse into rather than
    // treat as a leaf.
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
    must_succeed(!viewport.is_null(), "CreateWindowExW(CONTAINER_VIEWPORT)")?;

    // SAFETY: same reasoning as the `viewport` creation above; `viewport`
    // was just checked non-null and is the live parent HWND for this child.
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
        destroy_orphan(viewport, "CONTAINER_VIEWPORT");
        return Err(Error::windows_api("CreateWindowExW(CONTAINER_CONTENT)"));
    }

    if let Err(error) = registry.insert(node.id, NativeObject::Container { viewport, content }) {
        // Destroying the viewport would take the content window with it —
        // Win32 destroys a window's children — but both are named
        // explicitly so the intent survives any future change to which of
        // the two is the parent.
        destroy_orphan(content, "CONTAINER_CONTENT");
        destroy_orphan(viewport, "CONTAINER_VIEWPORT");
        return Err(error);
    }

    Ok(())
}

/// Hands a freshly created window to the registry, tearing it down if the
/// registry refuses it.
fn adopt(
    registry: &mut NativeObjectRegistry,
    node: &TreeNode,
    hwnd: HWND,
    wrap: fn(HWND) -> NativeObject,
    class_name: &'static str,
) -> Result<(), Error> {
    if hwnd.is_null() {
        return Err(Error::windows_api(match class_name {
            "STATIC" => "CreateWindowExW(STATIC)",
            "BUTTON" => "CreateWindowExW(BUTTON)",
            _ => "CreateWindowExW(EDIT)",
        }));
    }
    if let Err(error) = registry.insert(node.id, wrap(hwnd)) {
        destroy_orphan(hwnd, class_name);
        return Err(error);
    }
    Ok(())
}

/// Destroys a window that was created but never adopted by the registry, so
/// this call site is provably its only owner.
fn destroy_orphan(hwnd: HWND, what: &'static str) {
    // SAFETY: `hwnd` was created moments ago by this module and has not
    // been handed to the registry (every caller reaches here only on a
    // failure path before or during adoption), so nothing else holds it.
    let destroyed = unsafe { DestroyWindow(hwnd) } != 0;
    // Best effort: this is already an error path, and a leaked window
    // handle is a lesser problem than masking the error that led here.
    best_effort(destroyed, "DestroyWindow(orphan)", what);
}
