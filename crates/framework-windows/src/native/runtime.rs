//! `Runtime` owns one top-level window's state; `WindowRegistry` owns every
//! `Runtime` for one `run_application` call and keeps native windows in sync
//! with `Application::window_ids()`.

use std::collections::HashMap;

use framework_core::{Application, Event, NodeId, WindowId};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, PostMessageW, SW_SHOW, SetMenu, ShowWindow, WM_CLOSE,
    WS_OVERLAPPEDWINDOW,
};

use super::menu::build_native_menu;
use super::renderer::Renderer;
use super::util::{module_instance, wide};
use super::{EnableWindow, WINDOW_CLASS_NAME, WM_FRAMEWORK_SCHEDULE};
use crate::Error;

pub(crate) struct Runtime {
    pub(crate) application: *mut Application,
    pub(crate) window_id: WindowId,
    pub(crate) modal_parent: HWND,
    pub(crate) renderer: Renderer,
    pub(crate) window: HWND,
    pub(crate) focused: Option<NodeId>,
    pub(crate) error: Option<Error>,
    registry: *mut WindowRegistry,
    pub(crate) menu_commands: HashMap<u16, NodeId>,
    pub(crate) hovered: Option<NodeId>,
    pub(crate) pressed: Option<NodeId>,
    pub(crate) destroyed: bool,
}

impl Runtime {
    pub(crate) fn render(&mut self) -> Result<(), Error> {
        // SAFETY: `application` points to the mutable Application borrowed by
        // `WindowsPlatform::run` and remains valid for this event loop.
        let application = unsafe { &*self.application };
        let Some(tree) = application.view_for(self.window_id) else {
            return Ok(());
        };
        self.renderer.render(&tree, self.window, application.theme())
    }

    pub(crate) fn relayout(&mut self) {
        self.renderer.relayout(self.window);
    }

    pub(crate) fn dispatch(&mut self, event: Event) -> Result<(), Error> {
        // SAFETY: see `render`; the event loop has exclusive access to the
        // application while it is running.
        let application = unsafe { &mut *self.application };
        let handled = application.dispatch_to_window(self.window_id, event);

        // ComponentTree::dispatch updates component state and rebuilds the
        // framework tree. Reconcile that new tree back into native controls
        // immediately so the visible UI stays in sync with Rust state.
        if handled {
            self.render()?;
        }

        // A component may have queued a window-open or window-close
        // request while handling that event (see `ComponentContext::windows`).
        // Pick up any resulting change to the application's window set.
        self.sync_windows()
    }

    pub(crate) fn pump_tasks(&mut self) -> Result<(), Error> {
        // SAFETY: the runtime owns the application for the duration of the
        // native event loop.
        let application = unsafe { &mut *self.application };
        if application.pump_tasks_for(self.window_id) {
            self.render()?;
        }
        self.sync_windows()
    }

    /// Picks up any window opened or closed since the last sync. See
    /// `WindowRegistry::sync` for how each side is realized.
    fn sync_windows(&mut self) -> Result<(), Error> {
        if self.registry.is_null() {
            return Ok(());
        }
        // SAFETY: `registry` outlives every `Runtime` it owns; see
        // `run_application`.
        unsafe { &mut *self.registry }.sync()
    }
}

/// Owns every top-level `Runtime` for one `run_application` call and
/// keeps their native windows in sync with `Application::window_ids()`
/// as components open and close windows at runtime.
///
/// Runtimes are intentionally never removed from `runtimes` while the
/// message loop is running, even after their native window is destroyed:
/// a `Runtime` can be mid-dispatch (and so borrowed by a caller further
/// up the call stack) at the exact moment its own window is asked to
/// close, and removing it from this map would drop — and free — memory
/// that caller still holds a reference to. Instead, closing a window
/// posts it a `WM_CLOSE` (see `sync`) and lets its own, already-correct
/// `WM_CLOSE`/`WM_DESTROY` handling in `window_proc` tear it down on a
/// later, unnested turn of the message loop. Every `Runtime` — destroyed
/// or not — is finally dropped when `run_application` returns.
pub(crate) struct WindowRegistry {
    application: *mut Application,
    pub(crate) runtimes: HashMap<WindowId, Box<Runtime>>,
    // Reentrancy guard, belt-and-braces alongside registering in
    // `runtimes` as early as possible in `create_window`: covers the
    // (believed unreachable without `WS_VISIBLE`, but unverified on a
    // real Windows compiler — see BUILD_STATUS.md) case where a native
    // message is delivered synchronously even earlier than that, from
    // inside `CreateWindowExW` itself.
    creating: std::collections::HashSet<WindowId>,
}

impl WindowRegistry {
    pub(crate) fn new(application: *mut Application) -> Self {
        Self { application, runtimes: HashMap::new(), creating: std::collections::HashSet::new() }
    }

    pub(crate) fn sync(&mut self) -> Result<(), Error> {
        // SAFETY: `application` is the same pointer every `Runtime` here
        // already dereferences to dispatch events.
        let application = unsafe { &*self.application };
        let desired = application.window_ids();

        for (id, runtime) in &mut self.runtimes {
            if runtime.destroyed || desired.contains(id) {
                continue;
            }
            // Deferred: see the type-level doc comment on why this must
            // not destroy the window inline.
            runtime.destroyed = true;
            // SAFETY: `runtime.window` is a live HWND owned by this
            // `Runtime` for as long as it remains in `self.runtimes`
            // (see the type-level doc comment above); `PostMessageW`
            // takes no pointer arguments beyond the HWND itself.
            unsafe {
                PostMessageW(runtime.window, WM_CLOSE, 0, 0);
            }
        }

        for id in desired {
            if !self.runtimes.contains_key(&id) && !self.creating.contains(&id) {
                self.create_window(id)?;
            }
        }
        Ok(())
    }

    fn create_window(&mut self, id: WindowId) -> Result<(), Error> {
        // See the field doc comment on `creating`: this, together with
        // registering in `runtimes` as early as possible below, is what
        // stops a window from being created twice — which without this
        // guard is not a cosmetic bug but unbounded native window
        // creation, since each spurious window creation is itself
        // exactly the kind of event that triggers another `sync()`.
        if !self.creating.insert(id) {
            return Ok(());
        }
        let result = self.create_window_once(id);
        self.creating.remove(&id);
        result
    }

    fn create_window_once(&mut self, id: WindowId) -> Result<(), Error> {
        // SAFETY: see `sync`.
        let application = unsafe { &*self.application };
        let Some(definition) = application.window_for(id) else {
            return Ok(());
        };
        let title = wide(definition.title());
        let size = definition.size();
        let menu = definition.menu().cloned();
        let owner = application
            .window_state(id)
            .and_then(framework_core::WindowState::modal_parent)
            .and_then(|parent_id| self.runtimes.get(&parent_id))
            .map_or(std::ptr::null_mut(), |runtime| runtime.window);

        let registry_ptr: *mut WindowRegistry = self;
        let mut runtime = Box::new(Runtime {
            application: self.application,
            window_id: id,
            modal_parent: owner,
            renderer: Renderer::new(),
            window: std::ptr::null_mut(),
            focused: None,
            error: None,
            registry: registry_ptr,
            menu_commands: HashMap::new(),
            hovered: None,
            pressed: None,
            destroyed: false,
        });

        let runtime_ptr: *mut Runtime = &raw mut *runtime;
        // Window dimensions are always small, positive values in
        // practice (no real window is anywhere near `i32::MAX` wide or
        // tall); `Size`'s fields are `u32` only because layout
        // dimensions are never negative, not because they can exceed
        // `i32`'s range.
        #[allow(clippy::cast_possible_wrap)]
        let (width, height) = (size.width as i32, size.height as i32);
        // SAFETY: `wide(WINDOW_CLASS_NAME)` names the top-level window
        // class `register_window_classes` registers before any window
        // is created; its `Vec<u16>` is a temporary whose lifetime
        // Rust extends to the end of this statement, so the pointer
        // stays valid for the whole call; `title` is a NUL-terminated
        // wide buffer kept alive independently for the duration of this
        // call; `owner`, if non-null, is a live HWND from this
        // registry's own `runtimes`; `runtime_ptr` is later read back,
        // unchanged, from `WM_NCCREATE`'s `lpCreateParams` (see
        // `window_proc`'s `WM_NCCREATE` arm) and remains a valid
        // `*mut Runtime` for the window's whole lifetime since
        // `runtime` (boxed) is inserted into `self.runtimes`
        // immediately below and never moved out; a null return (checked
        // below) is `CreateWindowExW`'s documented failure signal.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                wide(WINDOW_CLASS_NAME).as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width,
                height,
                owner,
                std::ptr::null_mut(),
                module_instance(),
                runtime_ptr.cast(),
            )
        };
        if hwnd.is_null() {
            return Err(Error::windows_api("CreateWindowExW(top-level)"));
        }
        runtime.window = hwnd;

        // Register *before* any further native call. `CreateWindowExW`
        // above (and `ShowWindow` below) can synchronously deliver
        // messages — WM_SIZE in particular — to this window's own
        // `window_proc` before this function returns. Those handlers
        // dispatch through `Runtime::dispatch`, which calls back into
        // `sync()` on every dispatch (see its doc comment). If this
        // window were not yet in `self.runtimes` at that point, `sync()`
        // would see it as still missing from the native registry and
        // recurse into `create_window` for the same `WindowId` — which
        // creates *another* real native window, which can trigger the
        // same synchronous message, forever. This is not a hypothetical:
        // it is exactly what an earlier version of this function did.
        self.runtimes.insert(id, runtime);
        let runtime =
            self.runtimes.get_mut(&id).expect("just inserted this window's runtime above");

        if let Some(menu) = &menu {
            let built = build_native_menu(menu)?;
            // SAFETY: `hwnd` was just checked non-null above and is a
            // live top-level HWND; `built.handle()` is a live `HMENU` just
            // constructed by `build_native_menu`, not yet attached to
            // any window.
            if unsafe { SetMenu(hwnd, built.handle()) } == 0 {
                return Err(Error::windows_api("SetMenu"));
            }
            let (_, commands) = built.into_attached();
            runtime.menu_commands = commands;
        }

        let wake_target = hwnd as usize;
        // SAFETY: `wake_target` is `hwnd` captured as a plain integer
        // above; the target window, if it still exists by the time
        // this waker fires, is a valid HWND, and if it has since been
        // destroyed `PostMessageW` simply fails (a stale HWND is a
        // documented-safe, non-crashing input, never reused by Windows
        // for an unrelated live window within a single process's
        // lifetime in a way that would matter here).
        application.scheduler_for(id).expect("framework window must own a scheduler").set_waker(
            std::sync::Arc::new(move || unsafe {
                PostMessageW(wake_target as HWND, WM_FRAMEWORK_SCHEDULE, 0, 0);
            }),
        );

        runtime.render()?;
        // SAFETY: `hwnd` was checked non-null above and is a live,
        // just-created top-level HWND.
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
        }
        if !owner.is_null() {
            // SAFETY: `owner` was just checked non-null and is a live
            // HWND from this registry's own `runtimes`.
            unsafe {
                EnableWindow(owner, 0);
            }
        }

        Ok(())
    }
}
