//! The interaction-state machine (focus, hover, press) layered on top of
//! native controls.

use framework_core::{Event, NodeId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};

use super::super::runtime::Runtime;

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
                // A hidden screen's controls still exist, but Tab must not
                // wander into a screen nobody can see.
                && !runtime.renderer.snapshot.is_effectively_hidden(node.id)
                && runtime.renderer.registry.get(node.id).is_some()
        })
        .collect::<Vec<_>>();
    if focusable.is_empty() {
        return;
    }

    // Start from where focus *actually* is, not from the last value the
    // framework recorded. `runtime.focused` is refreshed by `sync_focus`
    // after a dispatched message, so the two normally agree — but native
    // focus can also move without one (application code calling `SetFocus`,
    // or the window manager activating the window), and traversing from a
    // stale position makes the first Tab appear to do nothing.
    let current = focused_node(runtime)
        .or(runtime.focused)
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
