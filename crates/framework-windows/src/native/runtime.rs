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

use super::context::HostRef;
use super::menu::build_native_menu;
use super::rendering::Renderer;
use super::util::{module_instance, wide};
use super::win32::{best_effort, ignored_by_contract, must_succeed};
use super::{EnableWindow, WINDOW_CLASS_NAME, WM_FRAMEWORK_SCHEDULE};
use crate::Error;
use crate::error::NativeContext;

/// One top-level window's native state, plus the two upward references it
/// needs to drive the framework: the `Application` whose view it renders,
/// and the `WindowRegistry` it asks to reconcile the window set after every
/// dispatch.
///
/// Both upward references are [`HostRef`]s rather than bare `*mut`s, and
/// every dereference of either goes through `HostRef::with` — see
/// `native::context`'s module documentation for the lifetime argument that
/// makes those sound, stated there once instead of at each use.
pub(crate) struct Runtime {
    application: HostRef<Application>,
    pub(crate) window_id: WindowId,
    pub(crate) modal_parent: HWND,
    pub(crate) renderer: Renderer,
    pub(crate) window: HWND,
    pub(crate) focused: Option<NodeId>,
    pub(crate) error: Option<Error>,
    registry: Option<HostRef<WindowRegistry>>,
    pub(crate) menu_commands: HashMap<u16, NodeId>,
    pub(crate) hovered: Option<NodeId>,
    pub(crate) pressed: Option<NodeId>,
    pub(crate) destroyed: bool,
}

impl Runtime {
    /// Runs `f` against the `Application` this window renders.
    ///
    /// Every use of the application from a `Runtime` funnels through here
    /// so the borrow is narrow by construction: `f` reads or mutates what it
    /// needs and returns, rather than holding a reference across a nested
    /// Win32 dispatch that could resolve the same `Application` again.
    pub(crate) fn with_application<R>(&self, f: impl FnOnce(&mut Application) -> R) -> R {
        // SAFETY: `self.application` was built from the `&mut Application`
        // `WindowsPlatform::run` holds for the whole message loop (point 1
        // of `native::context`'s module docs). No other borrow is live:
        // this method never calls itself, and each call site keeps `f`
        // free of further application access — see that module's contract
        // on `HostRef::with`.
        unsafe { self.application.with(f) }
    }

    pub(crate) fn render(&mut self) -> Result<(), Error> {
        let Some((tree, theme)) = self.with_application(|application| {
            application.view_for(self.window_id).map(|tree| (tree, application.theme().clone()))
        }) else {
            return Ok(());
        };
        self.renderer.render(&tree, self.window, &theme)
    }

    pub(crate) fn relayout(&mut self) {
        self.renderer.relayout(self.window);
    }

    /// [`Runtime::dispatch`], with this backend's uniform failure handling:
    /// a native error is recorded on the runtime and the message loop is
    /// asked to quit, and the return value says whether the caller should
    /// keep going.
    ///
    /// Every `WNDPROC` arm that dispatches an event needs exactly this, and
    /// each used to spell it out — an `if let Err`, an assignment to
    /// `runtime.error`, and its own `unsafe { PostQuitMessage(1) }` with its
    /// own SAFETY comment. Nine copies of a shutdown path is nine chances
    /// for one of them to drift, so it lives here once instead.
    ///
    /// The error is *stored* rather than returned because a `WNDPROC` cannot
    /// propagate one: it must return an `LRESULT` to Win32. `run_application`
    /// collects `Runtime::error` after the loop exits and surfaces it as the
    /// failure of `Platform::run`.
    pub(crate) fn dispatch_or_quit(&mut self, event: Event) -> bool {
        match self.dispatch(event) {
            Ok(()) => true,
            Err(error) => {
                self.error = Some(error);
                // SAFETY: `PostQuitMessage` takes a plain exit-code integer
                // and no pointer arguments; it only queues `WM_QUIT` so the
                // loop unwinds through its ordinary exit path.
                unsafe { windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(1) };
                false
            }
        }
    }

    pub(crate) fn dispatch(&mut self, event: Event) -> Result<(), Error> {
        let handled = self
            .with_application(|application| application.dispatch_to_window(self.window_id, event));

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
        if self.with_application(|application| application.pump_tasks_for(self.window_id)) {
            self.render()?;
        }
        self.sync_windows()
    }

    /// Picks up any window opened or closed since the last sync. See
    /// `WindowRegistry::sync` for how each side is realized.
    ///
    /// Normally reached at the tail of `dispatch`/`pump_tasks`. The panic
    /// boundary calls it directly (see
    /// `message_loop::poison_runtime_and_quit`) because a panic unwinds
    /// *past* that tail: the `CloseWindow` policy asks the application to
    /// close a window, and without a sync nothing would ever act on it.
    pub(crate) fn sync_windows(&mut self) -> Result<(), Error> {
        let Some(registry) = self.registry else {
            return Ok(());
        };
        // SAFETY: `registry` was built from the `&mut WindowRegistry`
        // `run_application` owns for the whole message loop (point 2 of
        // `native::context`'s module docs), so it outlives every `Runtime`
        // holding a copy. `WindowRegistry::sync` is the only thing reached
        // through this reference, and it cannot re-enter `sync_windows` on
        // the same registry in a way that observes a half-updated one: the
        // window creation it performs registers each new `Runtime` before
        // running any code that could dispatch (see `create_window_once`),
        // and the `creating` guard stops a nested `sync` from re-creating a
        // window mid-creation.
        unsafe { registry.with(WindowRegistry::sync) }
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
    application: HostRef<Application>,
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
    /// Borrows the `Application` for the whole message loop.
    ///
    /// # Safety
    ///
    /// `application` must outlive this registry, every `Runtime` it
    /// creates, and the message loop those run under — point 1 of
    /// `native::context`'s module documentation, which
    /// `native::app::run_application` upholds by construction.
    pub(crate) unsafe fn new(application: &mut Application) -> Self {
        Self {
            // SAFETY: forwarded from this function's own contract, just
            // above.
            application: unsafe { HostRef::new(application) },
            runtimes: HashMap::new(),
            creating: std::collections::HashSet::new(),
        }
    }

    /// Runs `f` against the borrowed `Application`. See
    /// [`Runtime::with_application`], which this mirrors for the registry's
    /// own copy of the same reference.
    fn with_application<R>(&self, f: impl FnOnce(&mut Application) -> R) -> R {
        // SAFETY: `self.application` is the same borrow every `Runtime`
        // this registry creates also holds, established by `new`'s
        // contract. Access is narrow and non-reentrant for the same reason
        // documented on `Runtime::with_application`.
        unsafe { self.application.with(f) }
    }

    pub(crate) fn sync(&mut self) -> Result<(), Error> {
        let desired = self.with_application(|application| application.window_ids());

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
            let posted = unsafe { PostMessageW(runtime.window, WM_CLOSE, 0, 0) } != 0;
            // Best effort rather than fatal: the only documented reason
            // this fails for a live window is the target thread's message
            // queue being full, and the window is already marked
            // `destroyed`, so the next `sync` simply skips it. Refusing to
            // open the *other* windows in `desired` below because one
            // close request could not be queued would be a worse outcome
            // than a window that lingers a moment longer.
            best_effort(posted, "PostMessageW(WM_CLOSE)", "the window is already marked closed");
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

    #[allow(
        clippy::expect_used,
        reason = "both sites read back state this same \n    /// function established a few lines earlier — the runtime it just inserted into \n    /// `self.runtimes`, and the scheduler `Application` creates with every window. \n    /// Neither is reachable from application input, so there is nothing for a `Result` \n    /// to hand back to"
    )]
    fn create_window_once(&mut self, id: WindowId) -> Result<(), Error> {
        let Some((title, size, menu, modal_parent)) = self.with_application(|application| {
            application.window_for(id).map(|definition| {
                (
                    wide(definition.title()),
                    definition.size(),
                    definition.menu().cloned(),
                    application
                        .window_state(id)
                        .and_then(framework_core::WindowState::modal_parent),
                )
            })
        }) else {
            return Ok(());
        };
        // Resolved after the application borrow ends rather than inside it:
        // the owner HWND comes from `self.runtimes`, and reading `self`
        // inside a closure that already borrows `self.application` would
        // need a second borrow of `self` for no benefit.
        let owner = modal_parent
            .and_then(|parent_id| self.runtimes.get(&parent_id))
            .map_or(std::ptr::null_mut(), |runtime| runtime.window);

        // SAFETY: `self` is borrowed mutably for this call and lives in
        // `run_application`'s stack frame for the whole message loop (point
        // 2 of `native::context`'s module docs), so it outlives every
        // `Runtime` this creates. `self` is not used through the original
        // borrow while a `Runtime` might dereference this copy: the only
        // access path is `Runtime::sync_windows`, whose own SAFETY comment
        // documents why that call cannot observe a half-updated registry.
        let registry = unsafe { HostRef::new(self) };
        let mut runtime = Box::new(Runtime {
            application: self.application,
            window_id: id,
            modal_parent: owner,
            renderer: Renderer::new(),
            window: std::ptr::null_mut(),
            focused: None,
            error: None,
            registry: Some(registry),
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
            return Err(Error::windows_api_in(
                "CreateWindowExW(top-level)",
                NativeContext::none().with_window(id),
            ));
        }
        runtime.window = hwnd;
        // Published before any further native call for the same reason
        // `self.runtimes.insert` below is: so a dialog request naming this
        // window as an owner (see `FileDialogRequest::owner`) can resolve
        // it as soon as the window exists, without a race against the
        // rest of this function's setup work.
        super::window_handles::set(id, hwnd);

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
            // The menu builder does not know which window it is building
            // for, so name it here — see `Error::or_context`.
            let built = build_native_menu(menu)
                .map_err(|error| error.or_context(NativeContext::none().with_window(id)))?;
            // SAFETY: `hwnd` was just checked non-null above and is a
            // live top-level HWND; `built.handle()` is a live `HMENU` just
            // constructed by `build_native_menu`, not yet attached to
            // any window.
            let attached = unsafe { SetMenu(hwnd, built.handle()) } != 0;
            // Must succeed: on failure `built` is dropped here, and its
            // `OwnedMenu` guard destroys the menu, so returning leaves no
            // leak — but continuing would leave a window whose menu items
            // are in `menu_commands` yet unreachable from any native menu.
            must_succeed(attached, "SetMenu").map_err(|error| {
                error.or_context(NativeContext::none().with_window(id).with_handle(hwnd as usize))
            })?;
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
        let waker = std::sync::Arc::new(move || {
            // SAFETY: as documented immediately above.
            //
            // A failed post means the window's message queue is full or the
            // window is gone; either way the wake is best effort, and the
            // next real message pumps the same tasks.
            let _ = unsafe { PostMessageW(wake_target as HWND, WM_FRAMEWORK_SCHEDULE, 0, 0) };
        });
        runtime.with_application(|application| {
            application
                .scheduler_for(id)
                .expect("framework window must own a scheduler")
                .set_waker(waker);
        });

        runtime
            .render()
            .map_err(|error| error.or_context(NativeContext::none().with_window(id)))?;
        // SAFETY: `hwnd` was checked non-null above and is a live,
        // just-created top-level HWND.
        //
        // `ShowWindow` returns whether the window was *previously* visible,
        // not whether the call worked, so there is no status here to check.
        ignored_by_contract(unsafe { ShowWindow(hwnd, SW_SHOW) });
        if !owner.is_null() {
            // SAFETY: `owner` was just checked non-null and is a live
            // HWND from this registry's own `runtimes`.
            //
            // Like `ShowWindow`, `EnableWindow` reports the window's
            // previous state rather than success.
            ignored_by_contract(unsafe { EnableWindow(owner, 0) });
        }

        Ok(())
    }
}
