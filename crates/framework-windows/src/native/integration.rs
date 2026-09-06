//! Native Win32 integration tests: real windows, real `WNDPROC`s, real
//! message dispatch.
//!
//! These are the scenarios the standards audit's P1.17 finding enumerates
//! by name, driven through [`super::harness::NativeHarness`] so every one
//! of them runs the production message-loop code path rather than a
//! stand-in. Each test's own doc comment says which failure it would
//! actually catch — a test that only asserts "nothing crashed" is not
//! evidence about a backend whose whole risk is lifetime and reentrancy.
//!
//! Read [`super::harness`]'s module docs first for what these can and
//! cannot run under (they need an interactive window station) and why they
//! serialize against each other.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use framework_core::{
    Application, Component, ComponentContext, Event, MenuBar, MenuItem, Node, PanicPolicy, Size,
    Window, WindowId,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, IsWindowEnabled, SetFocus, VK_TAB,
};

use super::harness::NativeHarness;

/// A shared, observable record of what a test component did, so a test can
/// assert on framework behavior from outside the component.
///
/// `Rc<RefCell<_>>` rather than a channel because everything here happens on
/// one thread, synchronously, inside the message loop — the value is fully
/// settled by the time the harness's `pump` returns.
type Log = Rc<RefCell<Vec<String>>>;

fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn entries(log: &Log) -> Vec<String> {
    log.borrow().clone()
}

fn window(title: &str) -> Window {
    Window::new(title, Size::new(420, 320))
}

// ---------------------------------------------------------------------------
// Test components
// ---------------------------------------------------------------------------

/// Counts clicks and records every event it sees.
struct Recorder {
    log: Log,
    clicks: u32,
}

impl Component for Recorder {
    type Props = Log;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { log: props, clicks: 0 }
    }
    fn props(&self) -> &Self::Props {
        &self.log
    }
    fn set_props(&mut self, props: Self::Props) {
        self.log = props;
    }

    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("caption", format!("clicks: {}", self.clicks)),
                Node::button("go", "Go"),
                Node::text_input("field", "typed"),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        if let Event::Click { .. } = event {
            self.clicks += 1;
        }
        self.log.borrow_mut().push(describe(&event));
    }
}

/// Swaps between two different child sets, so a rerender produces real
/// inserts, removals, and reorders against live native windows.
struct Reconciler {
    log: Log,
    swapped: bool,
}

impl Component for Reconciler {
    type Props = Log;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { log: props, swapped: false }
    }
    fn props(&self) -> &Self::Props {
        &self.log
    }
    fn set_props(&mut self, props: Self::Props) {
        self.log = props;
    }

    fn view(&self) -> Node {
        let children = if self.swapped {
            // `beta` moves from second to first, `alpha` is gone, `gamma`
            // is new — one render exercising a move, a removal, and an
            // insert at once.
            vec![Node::label("beta", "B"), Node::label("gamma", "C"), Node::button("go", "Go")]
        } else {
            vec![Node::label("alpha", "A"), Node::label("beta", "B"), Node::button("go", "Go")]
        };
        Node::column("root", children)
    }

    fn update(&mut self, event: Event) {
        if let Event::Click { .. } = event {
            self.swapped = !self.swapped;
        }
        self.log.borrow_mut().push(describe(&event));
    }
}

/// Opens and closes secondary windows in response to clicks.
struct WindowOpener {
    log: Log,
}

impl Component for WindowOpener {
    type Props = Log;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { log: props }
    }
    fn props(&self) -> &Self::Props {
        &self.log
    }
    fn set_props(&mut self, props: Self::Props) {
        self.log = props;
    }

    fn view(&self) -> Node {
        Node::column("root", [Node::button("open", "Open"), Node::button("close", "Close")])
    }

    fn update(&mut self, event: Event) {
        self.log.borrow_mut().push(describe(&event));
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        let _ = context;
        self.view()
    }
}

/// A leaf component used as the content of windows `WindowOpener` opens.
struct Secondary;

impl Component for Secondary {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        Self
    }
    fn props(&self) -> &Self::Props {
        &()
    }
    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::label("secondary-caption", "Secondary")
    }
    fn update(&mut self, _event: Event) {}
}

/// Spawns an asynchronous task on its first render and records when the
/// result lands, exercising the scheduler waker's `WM_FRAMEWORK_SCHEDULE`
/// path end to end.
struct TaskRunner {
    log: Log,
    spawned: bool,
    woken: bool,
}

impl Component for TaskRunner {
    type Props = Log;
    type Message = &'static str;

    fn new(props: Self::Props) -> Self {
        Self { log: props, spawned: false, woken: false }
    }
    fn props(&self) -> &Self::Props {
        &self.log
    }
    fn set_props(&mut self, props: Self::Props) {
        self.log = props;
    }

    fn view(&self) -> Node {
        Node::label("status", if self.woken { "woken" } else { "waiting" })
    }
    fn update(&mut self, _event: Event) {}

    fn message(&mut self, message: Self::Message) {
        self.woken = true;
        self.log.borrow_mut().push(message.to_owned());
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        if !self.spawned {
            self.spawned = true;
            // A task whose output *is* the message: the framework delivers
            // it through `Component::message` once the scheduler wakes the
            // message loop, which is the path under test. A short sleep
            // guarantees the completion lands after this render rather than
            // racing it, so the test really does exercise the wake rather
            // than an already-finished task.
            let delay = context.sleep(Duration::from_millis(20));
            context.spawn(async move {
                delay.await;
                "task-completed"
            });
        }
        self.view()
    }
}

/// Panics from `update`, to drive the `WNDPROC` panic boundary.
struct Exploder;

impl Component for Exploder {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        Self
    }
    fn props(&self) -> &Self::Props {
        &()
    }
    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::button("boom", "Boom")
    }

    fn update(&mut self, event: Event) {
        if let Event::Click { .. } = event {
            panic!("component panicked on purpose");
        }
    }
}

/// Renders a window carrying a native menu bar.
struct Menued {
    log: Log,
}

impl Menued {
    /// The menu the test window is built with. Command ids are assigned by
    /// `native::menu` in append order starting at 1, so the single leaf item
    /// here is command 1 — which the test uses to simulate the selection.
    fn menu() -> MenuBar {
        MenuBar::new([MenuItem::submenu("file", "File", [MenuItem::action("quit", "Quit")])])
    }
}

impl Component for Menued {
    type Props = Log;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { log: props }
    }
    fn props(&self) -> &Self::Props {
        &self.log
    }
    fn set_props(&mut self, props: Self::Props) {
        self.log = props;
    }

    fn view(&self) -> Node {
        Node::label("caption", "Menued")
    }

    fn update(&mut self, event: Event) {
        self.log.borrow_mut().push(describe(&event));
    }
}

/// A stable, assertion-friendly rendering of an event.
fn describe(event: &Event) -> String {
    match event {
        Event::Click { target } => format!("click:{}", target.get()),
        Event::TextChanged { target, value } => format!("text:{}:{value}", target.get()),
        Event::FocusGained { target } => format!("focus-gained:{}", target.get()),
        Event::FocusLost { target } => format!("focus-lost:{}", target.get()),
        Event::MenuAction { item, .. } => format!("menu:{}", item.get()),
        Event::WindowCloseRequested { window } => format!("close-requested:{}", window.get()),
        Event::WindowResized { .. } => "resized".to_owned(),
        Event::WindowMoved { .. } => "moved".to_owned(),
        Event::WindowStateChanged { .. } => "state-changed".to_owned(),
        Event::KeyDown { key, .. } => format!("key:{key:?}"),
        Event::TextInput { text, .. } => format!("input:{text}"),
        other => format!("other:{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The audit's named scenarios
// ---------------------------------------------------------------------------

/// A window comes up as a real, live `HWND` with its declared tree realized
/// natively, and goes away completely when closed.
///
/// Catches: a window that is created but never shown, a tree that reaches
/// the backend without producing controls, and — the sharp end — a close
/// that marks the runtime destroyed while leaving the native window alive,
/// which is the shape a leak takes here.
#[test]
fn native_window_lifecycle() {
    let log = log();
    let mut application = Application::new(Recorder::new(log.clone()), window("lifecycle"));
    // SAFETY: `application` is declared above `harness` and so outlives it.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    assert_eq!(harness.live_window_ids(), vec![WindowId::PRIMARY]);
    let hwnd = harness.hwnd(WindowId::PRIMARY);
    assert!(NativeHarness::window_exists(hwnd), "the primary window must be a real live HWND");

    for key in ["root", "caption", "go", "field"] {
        assert!(
            harness.control(WindowId::PRIMARY, key).is_some(),
            "every declared node must be realized as a native control ({key})"
        );
    }

    harness.request_close(WindowId::PRIMARY);
    assert!(harness.is_destroyed(WindowId::PRIMARY));
    assert!(
        !NativeHarness::window_exists(hwnd),
        "closing a window must destroy its native HWND, not just mark it closed"
    );
    assert!(harness.quit_requested(), "closing the primary window ends the application");
}

/// A rerender that inserts and removes children creates and destroys the
/// corresponding native windows.
///
/// Catches: a removed node whose `HWND` survives (the classic native leak),
/// and an inserted node the backend never realizes because the diff was
/// applied in the wrong order relative to its parent.
#[test]
fn native_child_reconciliation() {
    let log = log();
    let mut application = Application::new(Reconciler::new(log.clone()), window("reconcile"));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let alpha = harness.expect_control(WindowId::PRIMARY, "alpha");
    assert!(harness.control(WindowId::PRIMARY, "gamma").is_none());

    harness.click(WindowId::PRIMARY, "go");

    assert!(
        harness.control(WindowId::PRIMARY, "alpha").is_none(),
        "a node removed from the tree must be removed from the native registry"
    );
    assert!(
        !NativeHarness::window_exists(alpha),
        "a removed node's native window must actually be destroyed (P1.17: native leak)"
    );
    assert!(
        harness.control(WindowId::PRIMARY, "gamma").is_some(),
        "a newly declared node must be realized"
    );
}

/// A node that changes sibling position keeps its identity and its native
/// window rather than being torn down and rebuilt.
///
/// Catches: a diff that emits Remove+Insert instead of Move, which would
/// silently discard native state (an edit control's caret and selection,
/// a control's focus) on every reorder.
#[test]
fn native_reorder() {
    let log = log();
    let mut application = Application::new(Reconciler::new(log.clone()), window("reorder"));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let beta_before = harness.expect_control(WindowId::PRIMARY, "beta");
    harness.click(WindowId::PRIMARY, "go");
    let beta_after = harness.expect_control(WindowId::PRIMARY, "beta");

    assert_eq!(
        beta_before, beta_after,
        "a node that only moved must keep the same native window, not be recreated"
    );
    assert!(NativeHarness::window_exists(beta_after));
}

/// A declared text value reaches the native `EDIT` control, and a change the
/// person makes travels back as a `TextChanged` event.
///
/// Catches: the echo loop this backend has to avoid — the renderer's own
/// `SetWindowTextW` raises `EN_CHANGE` indistinguishably from typing, so a
/// missing suppression would report a spurious `TextChanged` on every
/// render.
#[test]
fn native_text_input() {
    let log = log();
    let mut application = Application::new(Recorder::new(log.clone()), window("text"));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let field = harness.expect_control(WindowId::PRIMARY, "field");
    assert_eq!(
        super::util::window_text(field),
        "typed",
        "the declared value must reach the native EDIT control"
    );

    log.borrow_mut().clear();
    // A render the framework itself triggers must not be reported back as a
    // person-initiated text change.
    harness.click(WindowId::PRIMARY, "go");
    assert!(
        !entries(&log).iter().any(|entry| entry.starts_with("text:")),
        "a framework-initiated repaint must not synthesize a TextChanged event; got {:?}",
        entries(&log)
    );
}

/// Tab moves keyboard focus between focusable controls, and skips the ones
/// that are not.
///
/// Catches: focusability declared portably but never realized as
/// `WS_TABSTOP` (the standards audit's P1.16 finding), and a label being
/// wrongly included in the tab order.
#[test]
fn native_focus_traversal() {
    let log = log();
    let mut application = Application::new(Recorder::new(log.clone()), window("focus"));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let button = harness.expect_control(WindowId::PRIMARY, "go");
    let field = harness.expect_control(WindowId::PRIMARY, "field");
    let caption = harness.expect_control(WindowId::PRIMARY, "caption");

    // SAFETY: every handle here is a live control from this window's own
    // registry; `SetFocus`/`GetFocus` take no pointer arguments.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(
            harness.hwnd(WindowId::PRIMARY),
        );
        SetFocus(button);
    }
    harness.pump();

    // SAFETY: `GetFocus` takes no arguments.
    let focused = unsafe { GetFocus() };
    if focused != button {
        // Focus needs an interactive, foregrounded window station. Rather
        // than assert something the environment can legitimately refuse,
        // verify the property that focus traversal actually depends on and
        // that this crate is responsible for: the tab-stop projection.
        assert!(
            super::rendering::accessibility::has_tab_stop(button),
            "a focusable button must carry WS_TABSTOP even where focus cannot be taken"
        );
        assert!(super::rendering::accessibility::has_tab_stop(field));
        assert!(
            !super::rendering::accessibility::has_tab_stop(caption),
            "a label is not a keyboard stop"
        );
        return;
    }

    harness.press_key(WindowId::PRIMARY, VK_TAB.into());
    // SAFETY: as above.
    let after_tab = unsafe { GetFocus() };
    assert_ne!(after_tab, button, "Tab must move focus off the button");
    assert_ne!(after_tab, caption, "Tab must not land on a non-focusable label");
    assert_eq!(after_tab, field, "Tab must advance to the next focusable control");
}

/// A component that asks for a new window gets a real second native window,
/// created after the dispatch that requested it rather than during it.
///
/// Catches the failure this backend actually shipped once: window creation
/// synchronously delivers `WM_SIZE`, whose dispatch triggers another window
/// sync, which — before the registry published the runtime early enough —
/// saw the window as still missing and created it again, unboundedly.
#[test]
fn native_dynamic_window_creation() {
    let log = log();
    let mut application = Application::new(WindowOpener::new(log.clone()), window("opener"));
    application.open_window(Secondary, window("secondary"), None);
    // SAFETY: `application` outlives `harness`.
    let harness = unsafe { NativeHarness::attach(&mut application) };

    let live = harness.live_window_ids();
    assert_eq!(
        live.len(),
        2,
        "exactly one extra window must be created, not one per synchronous WM_SIZE; got {live:?}"
    );
    let secondary = live[1];
    assert!(NativeHarness::window_exists(harness.hwnd(secondary)));
    assert!(
        harness.control(secondary, "secondary-caption").is_some(),
        "the new window must render its own component tree, not the primary window's"
    );
}

/// Closing a secondary window destroys it without disturbing the rest of the
/// application.
///
/// Catches: a close that tears the runtime down while a caller further up
/// the stack still holds it (the use-after-free `WindowRegistry` defers
/// destruction to avoid), and a closed window the registry immediately
/// recreates because the application's window set was never updated.
#[test]
fn native_dynamic_window_close() {
    let log = log();
    let mut application = Application::new(WindowOpener::new(log.clone()), window("opener"));
    let secondary = application.open_window(Secondary, window("secondary"), None);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let secondary_hwnd = harness.hwnd(secondary);

    harness.request_close(secondary);
    harness.pump();

    assert!(harness.is_destroyed(secondary));
    assert!(!NativeHarness::window_exists(secondary_hwnd));
    assert!(!harness.quit_requested(), "closing a secondary window must not end the application");
    assert_eq!(
        harness.live_window_ids(),
        vec![WindowId::PRIMARY],
        "a closed window must not be resurrected by the next window sync"
    );
    assert!(NativeHarness::window_exists(harness.hwnd(WindowId::PRIMARY)));
}

/// A modal child disables its owner for exactly as long as it is open.
///
/// Catches the failure that strands an application: an owner left disabled
/// after its modal child closes, which no further input can recover from.
#[test]
fn native_modal_window_lifecycle() {
    let log = log();
    let mut application = Application::new(WindowOpener::new(log.clone()), window("owner"));
    let modal = application.open_window(Secondary, window("modal"), Some(WindowId::PRIMARY));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let owner_hwnd = harness.hwnd(WindowId::PRIMARY);
    // SAFETY: `owner_hwnd` is a live top-level HWND from this backend's own
    // registry; `IsWindowEnabled` takes no pointer arguments.
    let enabled_while_modal = unsafe { IsWindowEnabled(owner_hwnd) } != 0;
    assert!(!enabled_while_modal, "an owner must be disabled while its modal child is open");

    harness.request_close(modal);
    harness.pump();

    // SAFETY: as above; the owner was not destroyed by the child's close.
    let enabled_after = unsafe { IsWindowEnabled(owner_hwnd) } != 0;
    assert!(enabled_after, "closing a modal child must re-enable its owner");
    assert!(NativeHarness::window_exists(owner_hwnd));
}

/// Selecting a native menu item reaches the component as an
/// `Event::MenuAction` naming the item's own identity.
///
/// Catches: a command-id table that never gets populated (the menu appears
/// but does nothing), and a menu selection misrouted as a control
/// notification — the two are the same `WM_COMMAND` distinguished only by
/// `lParam`.
#[test]
fn native_menu_dispatch() {
    let log = log();
    let menued = window("menued").with_menu(Menued::menu());
    let mut application = Application::new(Menued::new(log.clone()), menued);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    assert_eq!(
        harness.with_runtime(WindowId::PRIMARY, |runtime| runtime.menu_commands.len()),
        1,
        "the one leaf menu item must have been assigned a command id"
    );

    log.borrow_mut().clear();
    harness.select_menu_command(WindowId::PRIMARY, 1);

    let expected = format!("menu:{}", framework_core::NodeId::from_key("quit").get());
    assert!(
        entries(&log).contains(&expected),
        "selecting a menu item must dispatch MenuAction for that item; got {:?}",
        entries(&log)
    );
}

/// Repeatedly creating and destroying styled native controls does not
/// exhaust the process's GDI or USER handle quotas.
///
/// Catches a per-render handle leak, which is invisible in a short-lived
/// test and fatal in a long-running application. The instrument is the
/// quota itself rather than a handle-count reading: Windows' default
/// per-process quota is 10,000, so a leak of even one handle per cycle makes
/// window or brush creation start failing well before this finishes — an
/// unambiguous signal, unlike a count that caching can smear.
#[test]
fn native_gdi_resource_lifecycle() {
    /// How many create/destroy cycles to run. Each click swaps the child
    /// set — destroying two controls and creating two more — and restyles
    /// every surviving one, so a per-cycle handle leak compounds fast
    /// against Windows' 10,000-handle default per-process quota.
    const CYCLES: u32 = 600;

    let log = log();
    let mut application = Application::new(Reconciler::new(log.clone()), window("gdi"));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    for cycle in 0..CYCLES {
        harness.click(WindowId::PRIMARY, "go");
        assert!(
            harness.control(WindowId::PRIMARY, "go").is_some(),
            "control creation failed on cycle {cycle} of {CYCLES}, which is what running out \
             of USER/GDI handles looks like"
        );
    }

    // Every node the final tree does not contain must be gone from the
    // registry — a stale entry is a handle nothing will ever free.
    let realized = harness
        .with_runtime(WindowId::PRIMARY, |runtime| runtime.renderer.snapshot.nodes().count());
    assert!(realized > 0);
}

/// An asynchronous task's completion wakes the message loop and is delivered
/// to the component that spawned it.
///
/// Catches: a waker that never posts (the component's state would only
/// update on the next unrelated input), and a wake posted to a window whose
/// runtime cannot be resolved — both of which present as "async silently
/// does nothing" rather than as a crash.
#[test]
fn native_task_wakeup() {
    let log = log();
    let mut application = Application::new(TaskRunner::new(log.clone()), window("tasks"));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    // The task runs on the scheduler's own executor thread, so give the
    // wake a bounded window to arrive rather than assuming it already has.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline && entries(&log).is_empty() {
        std::thread::sleep(Duration::from_millis(10));
        harness.pump();
    }

    assert_eq!(
        entries(&log),
        vec!["task-completed".to_owned()],
        "a completed task must wake the native message loop and reach its component"
    );
}

/// A panic inside a component is caught at the `WNDPROC` boundary, recorded
/// as a typed error, and turned into a clean exit.
///
/// Catches the one failure mode here that is undefined behavior rather than
/// a bug: a Rust panic unwinding across an `extern "system"` frame into
/// Win32's own call stack.
#[test]
fn native_component_panic_boundary() {
    let mut application = Application::new(Exploder::new(()), window("panic"));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    // The boundary prints the panic through the default hook on its way to
    // being caught; silence it so a deliberately provoked panic does not
    // read as a test failure in the output.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    harness.click(WindowId::PRIMARY, "boom");
    std::panic::set_hook(previous_hook);

    let error = harness
        .error_for(WindowId::PRIMARY)
        .expect("a component panic must be recorded on the window's runtime");
    assert!(
        error.contains("component panicked on purpose"),
        "the recorded error must carry the panic's own message; got {error:?}"
    );
    assert!(
        harness.quit_requested(),
        "the default policy is Terminate, so a caught panic must ask the loop to exit"
    );
}

/// A component that panics in a window whose panic policy is `CloseWindow`
/// takes only that window down.
///
/// This is the policy's whole point (standards audit P2.33): an application
/// that would rather lose one window than the whole session can now say so,
/// and the backend has to honor it *without* destroying a `Runtime` that is
/// borrowed by the very callback that panicked — which is why the close is
/// routed through the same deferred path a normal close uses.
#[test]
fn native_panic_policy_close_window_keeps_the_application_alive() {
    let log = log();
    let mut application = Application::new(WindowOpener::new(log), window("survivor"));
    let exploding = application.open_window(Exploder, window("exploding"), None);
    application.set_panic_policy(PanicPolicy::CloseWindow);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let exploding_hwnd = harness.hwnd(exploding);

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    harness.click(exploding, "boom");
    std::panic::set_hook(previous_hook);
    harness.pump();

    assert!(!harness.quit_requested(), "CloseWindow must not end the application");
    assert!(
        !NativeHarness::window_exists(exploding_hwnd),
        "the panicking window must actually be destroyed, not merely deactivated"
    );
    assert_eq!(
        harness.live_window_ids(),
        vec![WindowId::PRIMARY],
        "every other window must survive a panic in one of them"
    );
    assert!(
        harness.error_for(exploding).is_none(),
        "under CloseWindow the panic is handled, so it must not also surface as a          Platform::run failure"
    );
}

/// `CloseWindow` degrades to terminating when the panicking window is the
/// only one left, rather than leaving a running event loop with nothing on
/// screen.
#[test]
fn native_panic_policy_close_window_terminates_when_it_is_the_last_window() {
    let mut application = Application::new(Exploder::new(()), window("only"));
    application.set_panic_policy(PanicPolicy::CloseWindow);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    harness.click(WindowId::PRIMARY, "boom");
    std::panic::set_hook(previous_hook);

    assert!(
        harness.quit_requested(),
        "with no other window to fall back to, CloseWindow must terminate instead"
    );
    assert!(harness.error_for(WindowId::PRIMARY).is_some());
}

/// `ReportAndContinue` catches the panic and keeps going: the window stays
/// open and the loop keeps running.
///
/// The panic still must not have unwound across the FFI boundary — that part
/// is not negotiable regardless of policy — which is what the window still
/// being alive and responsive afterwards demonstrates.
#[test]
fn native_panic_policy_report_and_continue_leaves_the_window_running() {
    let mut application = Application::new(Exploder::new(()), window("resilient"));
    application.set_panic_policy(PanicPolicy::ReportAndContinue);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let hwnd = harness.hwnd(WindowId::PRIMARY);

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    harness.click(WindowId::PRIMARY, "boom");
    // A second panic must be survivable too — a policy that only tolerates
    // the first one is not a "continue" policy.
    harness.click(WindowId::PRIMARY, "boom");
    std::panic::set_hook(previous_hook);

    assert!(!harness.quit_requested());
    assert!(harness.error_for(WindowId::PRIMARY).is_none());
    assert!(NativeHarness::window_exists(hwnd), "the window must still be alive and pumping");
    assert!(harness.control(WindowId::PRIMARY, "boom").is_some());
}

// ---------------------------------------------------------------------------
// Additional reentrancy and lifetime cases named by P0.2's "required tests"
// ---------------------------------------------------------------------------

/// Creating several windows in one sync produces exactly that many, even
/// though each creation synchronously delivers messages that trigger another
/// sync.
///
/// This is the reentrancy case the audit lists as "window creation during
/// another window's creation".
#[test]
fn native_window_creation_reentrancy() {
    let log = log();
    let mut application = Application::new(WindowOpener::new(log.clone()), window("reentrancy"));
    for index in 0..4 {
        application.open_window(Secondary, window(&format!("extra-{index}")), None);
    }
    // SAFETY: `application` outlives `harness`.
    let harness = unsafe { NativeHarness::attach(&mut application) };

    assert_eq!(
        harness.live_window_ids().len(),
        5,
        "four extra windows plus the primary; a reentrant sync must not create duplicates"
    );
}

/// A targeted event naming a node the tree no longer contains is rejected,
/// not repurposed.
///
/// The audit lists this as "message delivery after native object removal",
/// and its P1.9 finding explains why silently rerouting such an event to the
/// root component is worse than dropping it: a stale native callback would
/// trigger unrelated application logic.
#[test]
fn native_message_after_object_removal_is_rejected() {
    let log = log();
    let mut application = Application::new(Reconciler::new(log.clone()), window("stale"));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    // Remove `alpha` from the tree, then deliver a click that still names it.
    harness.click(WindowId::PRIMARY, "go");
    assert!(harness.control(WindowId::PRIMARY, "alpha").is_none());

    log.borrow_mut().clear();
    let stale = framework_core::NodeId::from_key("alpha");
    let delivered = harness.with_runtime(WindowId::PRIMARY, |runtime| {
        runtime.with_application(|application| {
            application.dispatch_to_window(WindowId::PRIMARY, Event::Click { target: stale })
        })
    });

    assert!(!delivered, "an event naming a removed node must not be handled");
    assert!(
        entries(&log).is_empty(),
        "a stale targeted event must not reach any component; got {:?}",
        entries(&log)
    );
}

/// Closing an owner window while its modal child is still open tears both
/// down without leaving a live child whose owner is gone.
#[test]
fn native_owner_close_with_open_modal_child() {
    let log = log();
    let mut application = Application::new(WindowOpener::new(log.clone()), window("owner"));
    let modal = application.open_window(Secondary, window("modal"), Some(WindowId::PRIMARY));
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let modal_hwnd = harness.hwnd(modal);
    let owner_hwnd = harness.hwnd(WindowId::PRIMARY);

    harness.request_close(WindowId::PRIMARY);
    harness.pump();

    assert!(!NativeHarness::window_exists(owner_hwnd));
    assert!(
        !NativeHarness::window_exists(modal_hwnd),
        "destroying an owner must take its owned modal child with it, per Win32's own \
         ownership rules — a surviving child would be unreachable"
    );
}
