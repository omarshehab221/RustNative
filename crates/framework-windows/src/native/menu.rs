//! Realizes a portable `MenuBar` as a native Win32 menu.

use std::collections::HashMap;

use framework_core::{MenuBar, MenuItem, NodeId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateMenu, CreatePopupMenu, DestroyMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING,
};

use super::util::wide;
use crate::Error;

/// A native menu realized from a portable `MenuBar`, together with the
/// command-id-to-`NodeId` table `window_proc` uses to translate a
/// `WM_COMMAND` menu selection back into an `Event::MenuAction`.
pub(crate) struct BuiltMenu {
    menu: OwnedMenu,
    commands: HashMap<u16, NodeId>,
}

/// Owns a menu until it is transferred to a top-level HWND. This makes
/// all menu-construction failure paths release nested Win32 resources.
struct OwnedMenu {
    handle: HMENU,
    attached: bool,
}

impl OwnedMenu {
    fn new(handle: HMENU) -> Self {
        Self { handle, attached: false }
    }
}

impl Drop for OwnedMenu {
    fn drop(&mut self) {
        if !self.attached && !self.handle.is_null() {
            // SAFETY: `self.handle` was just checked non-null, and
            // `self.attached` being false means ownership was never
            // transferred to a window via `SetMenu` (see
            // `into_attached`), so this `OwnedMenu` is still the sole
            // owner responsible for destroying it — exactly once, since
            // `Drop::drop` runs at most once.
            unsafe {
                DestroyMenu(self.handle);
            }
        }
    }
}

impl BuiltMenu {
    pub(crate) fn handle(&self) -> HMENU {
        self.menu.handle
    }

    pub(crate) fn into_attached(mut self) -> (HMENU, HashMap<u16, NodeId>) {
        self.menu.attached = true;
        (self.menu.handle, std::mem::take(&mut self.commands))
    }
}

pub(crate) fn build_native_menu(menu: &MenuBar) -> Result<BuiltMenu, Error> {
    // SAFETY: `CreateMenu` takes no arguments; a null return (checked
    // below) is its documented failure signal.
    let handle = unsafe { CreateMenu() };
    if handle.is_null() {
        return Err(Error::windows_api("CreateMenu"));
    }
    // Take ownership *before* the first fallible call below, not after the
    // loop succeeds. An earlier revision constructed the `OwnedMenu` only
    // in the success path, so a failure inside `append_menu_item` — an
    // out-of-resources `CreatePopupMenu`, a failed `AppendMenuW`, or
    // `Error::MenuCommandExhausted` — returned through `?` while `handle`
    // was still a bare `HMENU` owned by nothing, leaking the whole
    // partially built menu tree. That is precisely the failure mode the
    // standards audit's P1.14 finding calls out ("on intermediate failure,
    // the code returns an error without a clear ownership guard"), and it
    // is why ownership is established here rather than at the end.
    let owned = OwnedMenu::new(handle);
    let mut commands = HashMap::new();
    let mut next_command_id: u16 = 1;
    for item in menu.items() {
        append_menu_item(handle, item, &mut commands, &mut next_command_id)?;
    }
    Ok(BuiltMenu { menu: owned, commands })
}

fn append_menu_item(
    parent: HMENU,
    item: &MenuItem,
    commands: &mut HashMap<u16, NodeId>,
    next_command_id: &mut u16,
) -> Result<(), Error> {
    if item.is_separator() {
        // SAFETY: `parent` is a live `HMENU` owned by the caller
        // (either freshly created by `build_native_menu` or an ancestor
        // `submenu` created below, in either case not yet attached to a
        // window); `MF_SEPARATOR` ignores the `uIDNewItem`/`lpNewItem`
        // arguments, so the null `lpNewItem` here is valid.
        if unsafe { AppendMenuW(parent, MF_SEPARATOR, 0, std::ptr::null()) } == 0 {
            return Err(Error::windows_api("AppendMenuW(separator)"));
        }
        return Ok(());
    }

    let label = wide(item.label());
    if item.is_submenu() {
        // SAFETY: `CreatePopupMenu` takes no arguments; a null return
        // (checked below) is its documented failure signal.
        let submenu = unsafe { CreatePopupMenu() };
        if submenu.is_null() {
            return Err(Error::windows_api("CreatePopupMenu"));
        }
        let submenu_guard = OwnedMenu::new(submenu);
        for child in item.children() {
            append_menu_item(submenu, child, commands, next_command_id)?;
        }
        let mut flags = MF_STRING | MF_POPUP;
        if !item.is_enabled() {
            flags |= MF_GRAYED;
        }
        // SAFETY: `parent` is a live `HMENU` as above; `submenu` was
        // just checked non-null and, per `MF_POPUP`, is consumed as a
        // submenu handle rather than a plain item id; `label` is a
        // NUL-terminated wide buffer kept alive for the duration of
        // this call.
        if unsafe { AppendMenuW(parent, flags, submenu as usize, label.as_ptr()) } == 0 {
            return Err(Error::windows_api("AppendMenuW(submenu)"));
        }
        // The parent menu now owns the submenu and will destroy it.
        std::mem::forget(submenu_guard);
    } else {
        let command_id = *next_command_id;
        *next_command_id = next_command_id.checked_add(1).ok_or(Error::MenuCommandExhausted)?;
        commands.insert(command_id, item.id());

        let mut flags = MF_STRING;
        if !item.is_enabled() {
            flags |= MF_GRAYED;
        }
        if item.is_checked() == Some(true) {
            flags |= MF_CHECKED;
        }
        // SAFETY: `parent` is a live `HMENU` owned by the caller, as in
        // the branches above; `label` is a NUL-terminated wide buffer
        // kept alive for the duration of this call; `command_id`, per
        // plain `MF_STRING`, is consumed as a numeric item id, not a
        // pointer.
        if unsafe { AppendMenuW(parent, flags, command_id as usize, label.as_ptr()) } == 0 {
            return Err(Error::windows_api("AppendMenuW(item)"));
        }
    }
    Ok(())
}
