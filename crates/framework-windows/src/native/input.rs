//! Virtual-key translation and the interaction-state machine (focus, hover,
//! press) layered on top of native controls.

use framework_core::{Event, KeyCode, KeyModifiers, NodeId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyState, SetFocus, VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT,
    VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};

use super::runtime::Runtime;

pub(crate) fn key_code(vkey: u32) -> KeyCode {
    // `vkey` is documented by every caller (see `message_loop.rs`'s
    // `WM_KEYDOWN` handler) to come from a Win32 virtual-key code, which
    // Microsoft documents as always fitting in a `u16` (in practice,
    // almost always a single byte) — the `u32` parameter type exists only
    // because `WPARAM` is `usize`-sized, not because callers ever pass a
    // value this truncation could actually lose.
    #[allow(clippy::cast_possible_truncation)]
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
            KeyCode::Character(char::from_u32(u32::from(value)).unwrap_or('?'))
        }
        value => KeyCode::Unknown(u32::from(value)),
    }
}

pub(crate) fn modifiers() -> KeyModifiers {
    // SAFETY: `GetKeyState` takes a plain virtual-key-code integer and
    // no pointer arguments; it is always safe to call, from any thread.
    let shift = unsafe { GetKeyState(i32::from(VK_SHIFT)) } & i16::MIN != 0;
    // SAFETY: same as above.
    let ctrl = unsafe { GetKeyState(0x11) } & i16::MIN != 0;
    // SAFETY: same as above.
    let alt = unsafe { GetKeyState(0x12) } & i16::MIN != 0;
    KeyModifiers { shift, ctrl, alt }
}

pub(crate) fn focused_node(runtime: &Runtime) -> Option<NodeId> {
    // SAFETY: `GetFocus` takes no arguments; a null return (checked
    // below) is its documented "no focus in this thread's queue" signal.
    let focus = unsafe { GetFocus() };
    if focus.is_null() { None } else { runtime.renderer.registry.id_for_hwnd(focus) }
}

pub(crate) fn focus_next(runtime: &mut Runtime, backwards: bool) {
    let focusable = runtime
        .renderer
        .snapshot
        .ordered_nodes()
        .into_iter()
        .filter(|node| {
            node.accessibility.is_focusable()
                && !node.disabled
                && runtime.renderer.registry.get(node.id).is_some()
        })
        .collect::<Vec<_>>();
    if focusable.is_empty() {
        return;
    }

    let current = runtime.focused.and_then(|id| focusable.iter().position(|node| node.id == id));
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
        // SAFETY: `object.hwnd()` is a live HWND owned by this
        // renderer's registry.
        unsafe {
            SetFocus(object.hwnd());
        }
    }
}

pub(crate) fn sync_focus(runtime: &mut Runtime) {
    let next = focused_node(runtime);
    if next == runtime.focused {
        return;
    }

    if let Some(previous) = runtime.focused.take() {
        if let Err(error) = runtime.dispatch(Event::FocusLost { target: previous }) {
            runtime.error = Some(error);
            return;
        }
        runtime.renderer.set_control_state(previous, interaction_state(runtime, previous));
    }

    runtime.focused = next;
    if let Some(current) = next {
        if let Err(error) = runtime.dispatch(Event::FocusGained { target: current }) {
            runtime.error = Some(error);
        } else {
            runtime.renderer.set_control_state(current, interaction_state(runtime, current));
        }
    }
}

pub(crate) fn interaction_state(runtime: &Runtime, id: NodeId) -> framework_core::ControlState {
    if runtime.pressed == Some(id) {
        framework_core::ControlState::Pressed
    } else if runtime.hovered == Some(id) {
        framework_core::ControlState::Hovered
    } else if runtime.focused == Some(id) {
        framework_core::ControlState::Focused
    } else {
        framework_core::ControlState::Normal
    }
}

fn refresh_interaction_style(runtime: &mut Runtime, id: NodeId) {
    runtime.renderer.set_control_state(id, interaction_state(runtime, id));
}

pub(crate) fn set_hovered(runtime: &mut Runtime, next: Option<NodeId>) {
    let previous = runtime.hovered;
    if previous == next {
        return;
    }
    runtime.hovered = next;
    if let Some(previous) = previous {
        refresh_interaction_style(runtime, previous);
    }
    if let Some(current) = next {
        refresh_interaction_style(runtime, current);
    }
}

pub(crate) fn set_pressed(runtime: &mut Runtime, next: Option<NodeId>) {
    let previous = runtime.pressed;
    if previous == next {
        return;
    }
    runtime.pressed = next;
    if let Some(previous) = previous {
        refresh_interaction_style(runtime, previous);
    }
    if let Some(current) = next {
        refresh_interaction_style(runtime, current);
    }
}
