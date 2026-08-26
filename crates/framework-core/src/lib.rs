//! Platform-independent primitives for the framework.
//!
//! The core owns application components, the declarative UI tree, events, and
//! tree diffing. Platform crates translate the model into native objects; the
//! core never talks to an operating system.

use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};
use std::cell::RefCell;
use std::rc::Rc;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::thread;
use std::time::Duration;

/// Stable identity for a UI node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u64);

impl NodeId {
    /// Creates an ID from a stable application key.
    pub fn from_key(key: &str) -> Self {
        // FNV-1a. This is deterministic across processes and platforms; it is
        // an identity hash, not a cryptographic hash.
        let mut hash = 0xcbf29ce484222325u64;
        for byte in key.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Self(hash)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Input produced by a platform backend and delivered to the active component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Click { target: NodeId },
    FocusGained { target: NodeId },
    FocusLost { target: NodeId },
    KeyDown {
        target: Option<NodeId>,
        key: KeyCode,
        modifiers: KeyModifiers,
    },
    TextInput {
        target: Option<NodeId>,
        text: String,
    },
    TextChanged {
        target: NodeId,
        value: String,
    },
}

impl Event {
    pub const fn target(&self) -> Option<NodeId> {
        match self {
            Self::Click { target }
            | Self::FocusGained { target }
            | Self::FocusLost { target } => Some(*target),
            Self::KeyDown { target, .. } | Self::TextInput { target, .. } => *target,
            Self::TextChanged { target, .. } => Some(*target),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Enter,
    Space,
    Tab,
    Escape,
    Backspace,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Character(char),
    Unknown(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityRole {
    None,
    Label,
    Button,
    TextInput,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityInfo {
    pub role: AccessibilityRole,
    pub name: Option<String>,
    pub description: Option<String>,
    pub focusable: bool,
}

impl AccessibilityInfo {
    pub fn new(role: AccessibilityRole) -> Self {
        Self {
            role,
            name: None,
            description: None,
            focusable: false,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }
}

/// The application/component contract.
///
/// A component owns its state and describes how state becomes UI. Platform
/// backends only need to know how to feed framework events into this contract.
pub trait Component: 'static {
    type Props: Clone + PartialEq + 'static;
    type Message: Send + 'static;

    fn new(props: Self::Props) -> Self;
    fn props(&self) -> &Self::Props;
    fn set_props(&mut self, props: Self::Props);

    fn view(&self) -> Node;
    fn update(&mut self, event: Event);

    /// Handles typed messages emitted by child components through a Callback.
    fn message(&mut self, _message: Self::Message) {}

    /// Renders the component with access to the framework-managed child tree.
    /// Components that do not compose child components can rely on the default
    /// implementation, which simply calls `view()`.
    fn render(&mut self, _context: &mut ComponentContext<'_, Self::Message>) -> Node {
        self.view()
    }

    /// Called after the framework applies changed parent-provided props.
    fn props_changed(&mut self) {}

    /// Called when the component becomes part of the active component tree.
    fn mounted(&mut self) {}

    /// Called after the component handles an event and has a chance to update
    /// its state.
    fn updated(&mut self) {}

    /// Called when the component leaves the active component tree.
    fn unmounted(&mut self) {}
}

/// Stable identity for an entry in the managed component tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentId(u64);

impl ComponentId {
    pub const ROOT: Self = Self(0x6a09e667f3bcc909);

    fn child(parent: Self, key: &str) -> Self {
        let mut hash = 0xcbf29ce484222325u64 ^ parent.0;
        for byte in key.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Self(hash)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A render-time capability for composing stable child components.
pub struct ComponentContext<'a, M: Send + 'static> {
    tree: &'a mut ComponentTree,
    parent: ComponentId,
    generation: u64,
    task_scope: TaskScope,
    _message: std::marker::PhantomData<fn(M)>,
}

/// A typed child-to-parent message sender. Clones are cheap and represent the
/// same framework-managed channel endpoint.
#[derive(Clone)]
pub struct Callback<M: 'static> {
    target: ComponentId,
    sink: Rc<RefCell<VecDeque<QueuedMessage>>>,
    _marker: std::marker::PhantomData<fn(M)>,
}

impl<M: 'static> PartialEq for Callback<M> {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target && Rc::ptr_eq(&self.sink, &other.sink)
    }
}

impl<M: 'static> Eq for Callback<M> {}

impl<M: 'static> fmt::Debug for Callback<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Callback")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl<M: 'static> Callback<M> {
    pub fn send(&self, message: M) {
        self.sink.borrow_mut().push_back(QueuedMessage {
            target: self.target,
            message: Box::new(message),
        });
    }
}

struct QueuedMessage {
    target: ComponentId,
    message: Box<dyn Any>,
}

impl<'a, M: Send + 'static> ComponentContext<'a, M> {
    /// Creates a typed child-to-parent callback for this component.
    pub fn callback<Msg: 'static>(&self) -> Callback<Msg> {
        Callback {
            target: self.parent,
            sink: Rc::clone(&self.tree.message_sink),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn spawn<F>(&self, future: F) -> TaskHandle
    where
        F: Future<Output = M> + Send + 'static,
    {
        self.task_scope().spawn(future)
    }

    /// Returns the structured task scope owned by this component.
    ///
    /// The scope remains owned by the component even when this render-time
    /// handle is dropped. Tasks therefore survive renders but are automatically
    /// cancelled when the component leaves the managed tree.
    pub fn task_scope(&self) -> TaskScope {
        self.task_scope.clone()
    }

    pub fn sleep(&self, duration: Duration) -> SleepFuture {
        SleepFuture::new(duration)
    }

    /// Compose a child component with no meaningful inputs. Its props type must
    /// implement `Default`.
    pub fn child<C>(&mut self, key: impl Into<String>) -> Node
    where
        C: Component,
        C::Props: Default,
    {
        self.child_with_props(key, C::Props::default(), C::new)
    }

    /// Compose a child component with typed parent-provided inputs. The child
    /// instance is reused by key; changed props update the existing instance
    /// instead of remounting it.
    pub fn child_with_props<C, F>(
        &mut self,
        key: impl Into<String>,
        props: C::Props,
        constructor: F,
    ) -> Node
    where
        C: Component,
        F: FnOnce(C::Props) -> C,
    {
        self.tree
            .render_child_with_props::<C, F>(
                self.parent,
                key.into(),
                props,
                self.generation,
                constructor,
            )
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

#[derive(Debug, Clone)]
pub struct TaskHandle {
    id: TaskId,
    cancelled: Arc<AtomicBool>,
}

impl TaskHandle {
    pub fn id(&self) -> TaskId { self.id }
    pub fn cancel(&self) { self.cancelled.store(true, Ordering::Release); }
    pub fn is_cancelled(&self) -> bool { self.cancelled.load(Ordering::Acquire) }
}

/// Structured ownership of asynchronous tasks created by one component.
///
/// A scope is owned by a component-tree entry. Clones are lightweight render-time
/// handles to the same scope. When the component is unmounted, the framework
/// drops its owning scope and the shared scope state cancels every task.
#[derive(Clone)]
pub struct TaskScope {
    inner: Rc<TaskScopeInner>,
}

struct TaskScopeInner {
    scheduler: Scheduler,
    target: ComponentId,
    tasks: RefCell<Vec<TaskHandle>>,
}

impl fmt::Debug for TaskScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskScope")
            .field("target", &self.inner.target)
            .field("task_count", &self.task_count())
            .finish()
    }
}

impl TaskScope {
    fn new(scheduler: Scheduler, target: ComponentId) -> Self {
        Self {
            inner: Rc::new(TaskScopeInner {
                scheduler,
                target,
                tasks: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Spawns a task owned by this component scope.
    pub fn spawn<M, F>(&self, future: F) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + Send + 'static,
    {
        let handle = self.inner.scheduler.spawn(self.inner.target, future);
        self.inner.tasks.borrow_mut().push(handle.clone());
        handle
    }

    /// Cancels all currently owned tasks. This is idempotent.
    pub fn cancel_all(&self) {
        for task in self.inner.tasks.borrow().iter() {
            task.cancel();
        }
    }

    pub fn task_count(&self) -> usize {
        self.inner.tasks.borrow().len()
    }
}

impl Drop for TaskScopeInner {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().iter() {
            task.cancel();
        }
    }
}

struct CompletedTask {
    target: ComponentId,
    message: Box<dyn Any + Send>,
}

struct SchedulerInner {
    next_id: AtomicU64,
    completed: Mutex<VecDeque<CompletedTask>>,
    waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

#[derive(Clone)]
pub struct Scheduler { inner: Arc<SchedulerInner> }

impl Default for Scheduler {
    fn default() -> Self { Self::new() }
}

impl Scheduler {
    pub fn new() -> Self {
        Self { inner: Arc::new(SchedulerInner { next_id: AtomicU64::new(1), completed: Mutex::new(VecDeque::new()), waker: Mutex::new(None) }) }
    }

    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        *self.inner.waker.lock().expect("scheduler waker poisoned") = Some(waker);
    }

    pub fn spawn<M, F>(&self, target: ComponentId, future: F) -> TaskHandle
    where M: Send + 'static, F: Future<Output = M> + Send + 'static {
        let id = TaskId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_thread = Arc::clone(&cancelled);
        let inner = Arc::clone(&self.inner);
        thread::Builder::new().name(format!("framework-task-{}", id.0)).spawn(move || {
            let value = block_on(future);
            if cancelled_thread.load(Ordering::Acquire) { return; }
            inner.completed.lock().expect("scheduler queue poisoned").push_back(CompletedTask { target, message: Box::new(value) });
            if let Some(waker) = inner.waker.lock().expect("scheduler waker poisoned").clone() {
                waker();
            }
        }).expect("failed to spawn framework task");
        TaskHandle { id, cancelled }
    }

    fn drain(&self) -> Vec<CompletedTask> {
        let mut q = self.inner.completed.lock().expect("scheduler queue poisoned");
        q.drain(..).collect()
    }
}

pub struct SleepFuture {
    shared: Arc<(Mutex<bool>, Condvar)>,
    started: bool,
    duration: Duration,
}

impl SleepFuture {
    pub fn new(duration: Duration) -> Self { Self { shared: Arc::new((Mutex::new(false), Condvar::new())), started: false, duration } }
}

impl Future for SleepFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let (lock, _) = &*self.shared;
        if *lock.lock().expect("sleep state poisoned") { return Poll::Ready(()); }
        if !self.started {
            self.started = true;
            let shared = Arc::clone(&self.shared);
            let duration = self.duration;
            let waker = cx.waker().clone();
            thread::spawn(move || {
                thread::sleep(duration);
                let (lock, cv) = &*shared;
                *lock.lock().expect("sleep state poisoned") = true;
                cv.notify_all();
                waker.wake();
            });
        }
        Poll::Pending
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    struct Parker { ready: Mutex<bool>, cv: Condvar }
    let parker = Arc::new(Parker { ready: Mutex::new(false), cv: Condvar::new() });
    unsafe fn clone(data: *const ()) -> RawWaker {
        let arc = unsafe { Arc::<Parker>::from_raw(data as *const Parker) };
        let cloned = Arc::clone(&arc);
        std::mem::forget(arc);
        RawWaker::new(Arc::into_raw(cloned) as *const (), &VTABLE)
    }
    unsafe fn wake(data: *const ()) {
        let arc = unsafe { Arc::<Parker>::from_raw(data as *const Parker) };
        *arc.ready.lock().expect("task parker poisoned") = true;
        arc.cv.notify_one();
    }
    unsafe fn wake_by_ref(data: *const ()) {
        let arc = unsafe { Arc::<Parker>::from_raw(data as *const Parker) };
        *arc.ready.lock().expect("task parker poisoned") = true;
        arc.cv.notify_one();
        std::mem::forget(arc);
    }
    unsafe fn drop_waker(data: *const ()) { drop(unsafe { Arc::<Parker>::from_raw(data as *const Parker) }); }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop_waker);
    let raw = RawWaker::new(Arc::into_raw(Arc::clone(&parker)) as *const (), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw) };
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => {
                let mut ready = parker.ready.lock().expect("task parker poisoned");
                while !*ready { ready = parker.cv.wait(ready).expect("task parker poisoned"); }
                *ready = false;
            }
        }
    }
}


trait ManagedComponent {
    fn render(&mut self, tree: &mut ComponentTree, id: ComponentId, generation: u64, task_scope: TaskScope) -> Node;
    fn update(&mut self, event: Event);
    fn message_any(&mut self, message: Box<dyn Any>) -> bool;
    fn props_equal(&self, props: &dyn Any) -> bool;
    fn set_props_any(&mut self, props: Box<dyn Any>) -> bool;
    fn props_changed(&mut self);
    fn updated(&mut self);
    fn unmounted(&mut self);
}

impl<C: Component> ManagedComponent for C {
    fn render(&mut self, tree: &mut ComponentTree, id: ComponentId, generation: u64, task_scope: TaskScope) -> Node {
        let mut context = ComponentContext::<C::Message> {
            tree,
            parent: id,
            generation,
            task_scope,
            _message: std::marker::PhantomData,
        };
        Component::render(self, &mut context)
    }

    fn update(&mut self, event: Event) {
        Component::update(self, event);
    }

    fn message_any(&mut self, message: Box<dyn Any>) -> bool {
        let Ok(message) = message.downcast::<C::Message>() else {
            return false;
        };
        Component::message(self, *message);
        true
    }

    fn props_equal(&self, props: &dyn Any) -> bool {
        props
            .downcast_ref::<C::Props>()
            .map(|incoming| self.props() == incoming)
            .unwrap_or(false)
    }

    fn set_props_any(&mut self, props: Box<dyn Any>) -> bool {
        let Ok(props) = props.downcast::<C::Props>() else {
            return false;
        };
        Component::set_props(self, *props);
        true
    }

    fn props_changed(&mut self) {
        Component::props_changed(self);
    }

    fn updated(&mut self) {
        Component::updated(self);
    }

    fn unmounted(&mut self) {
        Component::unmounted(self);
    }

}

struct ComponentEntry {
    component: Box<dyn ManagedComponent>,
    component_type: std::any::TypeId,
    children: HashMap<String, ComponentId>,
    view: Option<Node>,
    used_generation: u64,
    task_scope: TaskScope,
}

/// Framework-owned keyed component tree.
///
/// Child components are created/reused during `render()`. A child that is no
/// longer requested by its parent is structurally removed and receives exactly
/// one `unmounted()` callback. Reappearing with the same key creates a new
/// instance, which receives a fresh `mounted()` callback.
pub struct ComponentTree {
    components: HashMap<ComponentId, ComponentEntry>,
    pending_children: HashMap<ComponentId, HashMap<String, ComponentId>>,
    node_owners: HashMap<NodeId, ComponentId>,
    generation: u64,
    root_view: Option<Node>,
    message_sink: Rc<RefCell<VecDeque<QueuedMessage>>>,
    scheduler: Scheduler,
}

impl ComponentTree {
    pub fn new<C: Component>(mut root: C) -> Self {
        root.mounted();
        let mut tree = Self {
            components: HashMap::new(),
            pending_children: HashMap::new(),
            node_owners: HashMap::new(),
            generation: 0,
            root_view: None,
            message_sink: Rc::new(RefCell::new(VecDeque::new())),
            scheduler: Scheduler::new(),
        };
        let root_scope = TaskScope::new(tree.scheduler.clone(), ComponentId::ROOT);
        tree.components.insert(
            ComponentId::ROOT,
            ComponentEntry {
                component_type: std::any::TypeId::of::<C>(),
                component: Box::new(root),
                children: HashMap::new(),
                view: None,
                used_generation: 0,
                task_scope: root_scope,
            },
        );
        tree.render();
        tree
    }

    pub fn render(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let root = self.render_component(ComponentId::ROOT, generation);
        self.root_view = Some(root);
        self.rebuild_node_owners();
    }

    pub fn view(&self) -> Node {
        self.root_view
            .clone()
            .expect("component tree must be rendered before its view is read")
    }

    pub fn dispatch(&mut self, event: Event) -> bool {
        let owner = event
            .target()
            .and_then(|target| self.node_owners.get(&target).copied())
            .unwrap_or(ComponentId::ROOT);

        let handled = self.update_component(owner, &event);

        // Child callbacks are transient runtime messages. They must be
        // delivered before the next declarative render so the render observes
        // the parent's updated state in the same event transaction.
        let had_messages = !self.message_sink.borrow().is_empty();
        if had_messages {
            self.drain_messages();
        }

        if handled || had_messages {
            self.render();
        }

        handled || had_messages
    }

    pub fn pump_tasks(&mut self) -> bool {
        let completed = self.scheduler.drain();
        if completed.is_empty() { return false; }
        let mut changed = false;
        for completed_task in completed {
            let Some(mut entry) = self.components.remove(&completed_task.target) else { continue; };
            if entry.component.message_any(completed_task.message) {
                entry.component.updated();
                changed = true;
            }
            self.components.insert(completed_task.target, entry);
        }
        if changed { self.render(); }
        changed
    }

    pub fn scheduler(&self) -> &Scheduler { &self.scheduler }

    fn task_scope(&self, id: ComponentId) -> TaskScope {
        self.components
            .get(&id)
            .map(|entry| entry.task_scope.clone())
            .expect("component task scope must exist")
    }

    fn drain_messages(&mut self) {
        loop {
            let next = self.message_sink.borrow_mut().pop_front();
            let Some(QueuedMessage { target, message }) = next else { break; };

            let Some(mut entry) = self.components.remove(&target) else { continue; };
            if entry.component.message_any(message) {
                entry.component.updated();
                self.components.insert(target, entry);
            } else {
                self.components.insert(target, entry);
            }
        }
    }

    fn render_child_with_props<C, F>(
        &mut self,
        parent: ComponentId,
        key: String,
        props: C::Props,
        generation: u64,
        constructor: F,
    ) -> Node
    where
        C: Component,
        F: FnOnce(C::Props) -> C,
    {
        let existing_id = self
            .pending_children
            .get(&parent)
            .and_then(|children| children.get(&key).copied());

        let needs_create = match existing_id {
            Some(existing_id) => self
                .components
                .get(&existing_id)
                .map(|entry| entry.component_type != std::any::TypeId::of::<C>())
                .unwrap_or(true),
            None => true,
        };

        let id = ComponentId::child(parent, &key);

        if needs_create {
            if let Some(existing_id) = existing_id {
                self.remove_component(existing_id);
            }

            let mut component = constructor(props);
            component.mounted();
            self.components.insert(
                id,
                ComponentEntry {
                    component: Box::new(component),
                    component_type: std::any::TypeId::of::<C>(),
                    children: HashMap::new(),
                    view: None,
                    used_generation: generation,
                    task_scope: TaskScope::new(self.scheduler.clone(), id),
                },
            );
            self.pending_children
                .entry(parent)
                .or_default()
                .insert(key.clone(), id);
        } else {
            let changed = self
                .components
                .get(&id)
                .map(|entry| !entry.component.props_equal(&props as &dyn Any))
                .unwrap_or(false);

            if changed {
                let mut entry = self
                    .components
                    .remove(&id)
                    .expect("managed child must exist when updating its props");
                let applied = entry.component.set_props_any(Box::new(props));
                if applied {
                    entry.component.props_changed();
                }
                self.components.insert(id, entry);
            }

            self.pending_children
                .entry(parent)
                .or_default()
                .insert(key.clone(), id);
        }

        self.render_component(id, generation)
    }

    fn render_component(&mut self, id: ComponentId, generation: u64) -> Node {
        let mut entry = self
            .components
            .remove(&id)
            .expect("component tree entry must exist while rendering");

        entry.used_generation = generation;
        let task_scope = entry.task_scope.clone();
        self.pending_children.insert(id, entry.children.clone());
        let node = entry.component.render(self, id, generation, task_scope);
        entry.view = Some(node.clone());
        entry.children = self.pending_children.remove(&id).unwrap_or_default();
        self.components.insert(id, entry);

        self.prune_unused_children(id, generation);
        node
    }

    fn rebuild_node_owners(&mut self) {
        self.node_owners.clear();
        self.collect_node_owners(ComponentId::ROOT);
    }

    fn collect_node_owners(&mut self, id: ComponentId) {
        let (children, view) = match self.components.get(&id) {
            Some(entry) => (
                entry.children.values().copied().collect::<Vec<_>>(),
                entry.view.clone(),
            ),
            None => return,
        };

        // Children are registered first so a parent's view cannot claim a
        // descendant that belongs to a more specific component.
        for child in children {
            self.collect_node_owners(child);
        }

        if let Some(view) = view {
            view.visit(&mut |node, _, _| {
                self.node_owners.entry(node.id()).or_insert(id);
            });
        }
    }

    fn prune_unused_children(&mut self, parent: ComponentId, generation: u64) {
        let stale = self
            .components
            .get(&parent)
            .map(|entry| {
                entry
                    .children
                    .values()
                    .copied()
                    .filter(|id| {
                        self.components
                            .get(id)
                            .map(|child| child.used_generation != generation)
                            .unwrap_or(false)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if stale.is_empty() {
            return;
        }

        if let Some(entry) = self.components.get_mut(&parent) {
            entry.children.retain(|_, id| !stale.contains(id));
        }

        for id in stale {
            self.remove_component(id);
        }
    }

    fn remove_component(&mut self, id: ComponentId) {
        if id == ComponentId::ROOT {
            return;
        }

        let Some(mut entry) = self.components.remove(&id) else {
            return;
        };

        // Structured task ownership ends with the component lifetime. Cancel
        // before unmounting so no task can legitimately outlive its owner.
        entry.task_scope.cancel_all();

        let children = entry.children.values().copied().collect::<Vec<_>>();
        for child in children {
            self.remove_component(child);
        }

        entry.component.unmounted();
    }

    fn update_component(&mut self, id: ComponentId, event: &Event) -> bool {
        let Some(mut entry) = self.components.remove(&id) else {
            return false;
        };

        entry.component.update(event.clone());
        entry.component.updated();
        self.components.insert(id, entry);
        true
    }
}

impl Drop for ComponentTree {
    fn drop(&mut self) {
        if self.components.contains_key(&ComponentId::ROOT) {
            self.remove_component_children(ComponentId::ROOT);
            if let Some(mut root) = self.components.remove(&ComponentId::ROOT) {
                root.component.unmounted();
            }
        }
    }
}

impl ComponentTree {
    fn remove_component_children(&mut self, parent: ComponentId) {
        let children = self
            .components
            .get(&parent)
            .map(|entry| entry.children.values().copied().collect::<Vec<_>>())
            .unwrap_or_default();

        for child in children {
            self.remove_component_children(child);
            if let Some(mut entry) = self.components.remove(&child) {
                entry.component.unmounted();
            }
        }
    }
}

/// A stable slot for composing one component directly. Kept for low-level
/// ownership scenarios; application code should prefer `ComponentContext::child`.
pub struct ComponentHost<C: Component> {
    component: C,
}

impl<C: Component> ComponentHost<C> {
    pub fn new(mut component: C) -> Self {
        component.mounted();
        Self { component }
    }

    pub fn is_mounted(&self) -> bool { true }
    pub fn view(&self) -> Node { self.component.view() }

    pub fn update(&mut self, event: Event) -> bool {
        if !self.owns_event(&event) { return false; }
        self.component.update(event);
        self.component.updated();
        true
    }

    pub fn owns_event(&self, event: &Event) -> bool {
        match event.target() {
            Some(target) => self.component.view().contains_id(target),
            None => true,
        }
    }

    pub fn component(&self) -> &C { &self.component }
    pub fn component_mut(&mut self) -> &mut C { &mut self.component }

    pub fn replace(&mut self, mut component: C) {
        self.component.unmounted();
        component.mounted();
        self.component = component;
    }
}

impl<C: Component> Drop for ComponentHost<C> {
    fn drop(&mut self) { self.component.unmounted(); }
}

pub struct Application {
    window: Window,
    components: ComponentTree,
}

impl fmt::Debug for Application {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Application")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

impl Application {
    pub fn new<C: Component>(component: C, window: Window) -> Self {
        Self {
            window,
            components: ComponentTree::new(component),
        }
    }

    pub fn dispatch(&mut self, event: Event) -> bool {
        self.components.dispatch(event)
    }

    pub fn view(&self) -> Node { self.components.view() }
    pub fn render(&mut self) { self.components.render(); }
    pub fn components(&self) -> &ComponentTree { &self.components }
    pub fn pump_tasks(&mut self) -> bool { self.components.pump_tasks() }
    pub fn scheduler(&self) -> &Scheduler { self.components.scheduler() }

    pub fn window(&self) -> &Window { &self.window }
}

#[derive(Debug, Clone)]
pub struct Window {
    title: String,
    size: Size,
}

impl Window {
    pub fn new(title: impl Into<String>, size: Size) -> Self {
        Self {
            title: title.into(),
            size,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn size(&self) -> Size {
        self.size
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeMode {
    Auto,
    Fixed(i32),
    Fill,
}

impl Default for SizeMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Start,
    Center,
    End,
    Stretch,
}

impl Default for Alignment {
    fn default() -> Self {
        Self::Stretch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self { Self { x, y } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Clip,
    Scroll,
}

impl Default for Overflow {
    fn default() -> Self { Self::Clip }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeInsets {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl EdgeInsets {
    pub const fn all(value: i32) -> Self {
        Self { top: value, right: value, bottom: value, left: value }
    }

    pub const fn symmetric(vertical: i32, horizontal: i32) -> Self {
        Self { top: vertical, right: horizontal, bottom: vertical, left: horizontal }
    }

    pub const fn horizontal(self) -> i32 {
        self.left + self.right
    }

    pub const fn vertical(self) -> i32 {
        self.top + self.bottom
    }
}

impl Default for EdgeInsets {
    fn default() -> Self {
        Self::all(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Constraints {
    pub min_width: i32,
    pub max_width: Option<i32>,
    pub min_height: i32,
    pub max_height: Option<i32>,
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            min_width: 0,
            max_width: None,
            min_height: 0,
            max_height: None,
        }
    }
}

impl Constraints {
    pub const fn new() -> Self { Self { min_width: 0, max_width: None, min_height: 0, max_height: None } }
    pub const fn min_width(mut self, value: i32) -> Self { self.min_width = if value > 0 { value } else { 0 }; self }
    pub const fn max_width(mut self, value: i32) -> Self { self.max_width = Some(if value > 0 { value } else { 0 }); self }
    pub const fn min_height(mut self, value: i32) -> Self { self.min_height = if value > 0 { value } else { 0 }; self }
    pub const fn max_height(mut self, value: i32) -> Self { self.max_height = Some(if value > 0 { value } else { 0 }); self }

    pub const fn clamp_width(self, value: i32) -> i32 {
        let mut value = if value < self.min_width { self.min_width } else { value };
        if let Some(max) = self.max_width {
            let max = if max < self.min_width { self.min_width } else { max };
            if value > max { value = max; }
        }
        value
    }

    pub const fn clamp_height(self, value: i32) -> i32 {
        let mut value = if value < self.min_height { self.min_height } else { value };
        if let Some(max) = self.max_height {
            let max = if max < self.min_height { self.min_height } else { max };
            if value > max { value = max; }
        }
        value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutStyle {
    pub width: SizeMode,
    pub height: SizeMode,
    pub margin: EdgeInsets,
    pub align_self: Option<Alignment>,
    pub constraints: Constraints,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            width: SizeMode::Fill,
            height: SizeMode::Auto,
            margin: EdgeInsets::default(),
            align_self: None,
            constraints: Constraints::default(),
        }
    }
}

impl LayoutStyle {
    pub const fn new() -> Self {
        Self {
            width: SizeMode::Fill,
            height: SizeMode::Auto,
            margin: EdgeInsets { top: 0, right: 0, bottom: 0, left: 0 },
            align_self: None,
            constraints: Constraints::new(),
        }
    }

    pub const fn width(mut self, width: SizeMode) -> Self {
        self.width = width;
        self
    }

    pub const fn height(mut self, height: SizeMode) -> Self {
        self.height = height;
        self
    }

    pub const fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = margin;
        self
    }

    pub const fn align_self(mut self, alignment: Alignment) -> Self {
        self.align_self = Some(alignment);
        self
    }

    pub const fn constraints(mut self, constraints: Constraints) -> Self {
        self.constraints = constraints;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnStyle {
    pub padding: EdgeInsets,
    pub gap: i32,
    pub align_items: Alignment,
    pub overflow: Overflow,
}

impl Default for ColumnStyle {
    fn default() -> Self {
        Self {
            padding: EdgeInsets::all(24),
            gap: 12,
            align_items: Alignment::Stretch,
            overflow: Overflow::Clip,
        }
    }
}

impl ColumnStyle {
    pub const fn new() -> Self {
        Self {
            padding: EdgeInsets { top: 24, right: 24, bottom: 24, left: 24 },
            gap: 12,
            align_items: Alignment::Stretch,
            overflow: Overflow::Clip,
        }
    }

    pub const fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    pub const fn gap(mut self, gap: i32) -> Self {
        self.gap = gap;
        self
    }

    pub const fn align_items(mut self, alignment: Alignment) -> Self {
        self.align_items = alignment;
        self
    }

    pub const fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowStyle {
    pub padding: EdgeInsets,
    pub gap: i32,
    pub align_items: Alignment,
    pub overflow: Overflow,
}

impl Default for RowStyle {
    fn default() -> Self {
        Self {
            padding: EdgeInsets::all(24),
            gap: 12,
            align_items: Alignment::Stretch,
            overflow: Overflow::Clip,
        }
    }
}

impl RowStyle {
    pub const fn new() -> Self {
        Self {
            padding: EdgeInsets { top: 24, right: 24, bottom: 24, left: 24 },
            gap: 12,
            align_items: Alignment::Stretch,
            overflow: Overflow::Clip,
        }
    }

    pub const fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    pub const fn gap(mut self, gap: i32) -> Self {
        self.gap = gap;
        self
    }

    pub const fn align_items(mut self, alignment: Alignment) -> Self {
        self.align_items = alignment;
        self
    }

    pub const fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }
}

/// Provides platform-specific intrinsic measurements for leaf nodes.
pub trait IntrinsicMeasurer {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size;
}

#[derive(Debug, Default)]
pub struct DefaultIntrinsicMeasurer;

impl IntrinsicMeasurer for DefaultIntrinsicMeasurer {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        let characters = text.map(|value| value.chars().count()).unwrap_or(0) as i32;
        let intrinsic_width = (characters * 8
            + match kind {
                NodeKind::Button | NodeKind::TextInput => 24,
                NodeKind::Label => 0,
                NodeKind::Column | NodeKind::Row => 0,
            })
            .max(1);
        let available_width = max_width.unwrap_or(intrinsic_width).max(1);
        let lines = ((intrinsic_width + available_width - 1) / available_width).max(1);
        let width = intrinsic_width.min(available_width);

        Size::new(width as u32, (lines * 32) as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutInvalidation {
    pub full: bool,
}

impl LayoutInvalidation {
    pub const fn none() -> Self { Self { full: false } }
    pub const fn full() -> Self { Self { full: true } }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayoutResult {
    pub rects: HashMap<NodeId, Rect>,
    pub clips: HashMap<NodeId, Rect>,
    pub content_sizes: HashMap<NodeId, Size>,
    pub scroll_ranges: HashMap<NodeId, Size>,
}

#[derive(Debug, Default)]
pub struct LayoutEngine;

impl LayoutEngine {
    pub const fn new() -> Self { Self }

    pub fn layout(&self, snapshot: &TreeSnapshot, size: Size) -> HashMap<NodeId, Rect> {
        self.layout_with(snapshot, size, &DefaultIntrinsicMeasurer)
    }

    pub fn layout_with<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        size: Size,
        measurer: &M,
    ) -> HashMap<NodeId, Rect> {
        self.layout_result_with(snapshot, size, measurer, &HashMap::new()).rects
    }

    pub fn layout_result_with<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        size: Size,
        measurer: &M,
        _scroll_offsets: &HashMap<NodeId, Point>,
    ) -> LayoutResult {
        let mut result = LayoutResult::default();
        let Some(root) = snapshot.nodes().find(|node| node.parent.is_none()) else {
            return result;
        };

        let root_rect = Rect::new(
            0,
            0,
            size.width.min(i32::MAX as u32) as i32,
            size.height.min(i32::MAX as u32) as i32,
        );

        self.layout_node(snapshot, root, root_rect, &mut result, measurer);
        result
    }

    fn layout_node<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        rect: Rect,
        result: &mut LayoutResult,
        measurer: &M,
    ) {
        // Rectangles are always expressed in the coordinate space of the
        // node's native parent. Descendants therefore start from (0, 0)
        // inside their own native container instead of inheriting the
        // parent's absolute position.
        result.rects.insert(node.id, rect);
        let content_rect = Rect::new(0, 0, rect.width, rect.height);

        let mut children = snapshot
            .nodes()
            .filter(|candidate| candidate.parent == Some(node.id))
            .collect::<Vec<_>>();
        children.sort_by_key(|child| child.index);

        match node.kind {
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                if !matches!(style.overflow, Overflow::Visible) {
                    result.clips.insert(node.id, Rect::new(0, 0, rect.width, rect.height));
                }
                let content_size = self.layout_column(
                    snapshot, node, content_rect, children, style.padding, style.gap,
                    style.align_items, result, measurer,
                );
                result.content_sizes.insert(node.id, content_size);
                result.scroll_ranges.insert(node.id, Size::new(
                    content_size.width.saturating_sub(rect.width.max(0) as u32),
                    content_size.height.saturating_sub(rect.height.max(0) as u32),
                ));
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                if !matches!(style.overflow, Overflow::Visible) {
                    result.clips.insert(node.id, Rect::new(0, 0, rect.width, rect.height));
                }
                let content_size = self.layout_row(
                    snapshot, node, content_rect, children, style.padding, style.gap,
                    style.align_items, result, measurer,
                );
                result.content_sizes.insert(node.id, content_size);
                result.scroll_ranges.insert(node.id, Size::new(
                    content_size.width.saturating_sub(rect.width.max(0) as u32),
                    content_size.height.saturating_sub(rect.height.max(0) as u32),
                ));
            }
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {}
        }
    }

    fn layout_column<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        _node: &TreeNode,
        rect: Rect,
        children: Vec<&TreeNode>,
        padding: EdgeInsets,
        gap: i32,
        align_items: Alignment,
        result: &mut LayoutResult,
        measurer: &M,
    ) -> Size {
        let content = inner_rect(rect, padding);
        if children.is_empty() {
            return Size::new(padding.horizontal().max(0) as u32, padding.vertical().max(0) as u32);
        }

        let gap_total = gap.max(0) * children.len().saturating_sub(1) as i32;
        let usable_height = (content.height - gap_total).max(0);
        let preferred_height = children.iter().map(|child| self.preferred_height(snapshot, child, measurer, preferred_width_hint(child))).sum::<i32>();
        let fill_count = children.iter().filter(|child| matches!(child.layout.height, SizeMode::Fill)).count();
        let distributable = (usable_height - preferred_height).max(0);
        let share = if fill_count == 0 { 0 } else { distributable / fill_count as i32 };
        let remainder = if fill_count == 0 { 0 } else { distributable % fill_count as i32 };

        let mut y = content.y;
        let mut fill_index = 0;
        for (index, child) in children.iter().enumerate() {
            let margin = child.layout.margin;
            y += margin.top;
            let height = match child.layout.height {
                SizeMode::Fixed(value) => value.max(0),
                SizeMode::Auto => self.preferred_height(snapshot, child, measurer, preferred_width_hint(child)),
                SizeMode::Fill => {
                    let extra = if fill_index == 0 { remainder } else { 0 };
                    fill_index += 1;
                    (self.preferred_height(snapshot, child, measurer, preferred_width_hint(child)) + share + extra).max(0)
                }
            };

            let available_width = (content.width - margin.horizontal()).max(0);
            let alignment = child.layout.align_self.unwrap_or(align_items);
            let width = match (alignment, child.layout.width) {
                (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available_width,
                (_, SizeMode::Auto) => self.preferred_width(snapshot, child, measurer).min(available_width),
                (_, mode) => resolve_width(mode, available_width),
            };
            let width = child.layout.constraints.clamp_width(width.max(0)).min(available_width.max(0));
            let height = child.layout.constraints.clamp_height(height.max(0));
            let x = aligned_start(content.x, margin.left, available_width, width, alignment);
            let child_rect = Rect::new(x, y, width, height);
            self.layout_node(snapshot, child, child_rect, result, measurer);

            y += height + margin.bottom;
            if index + 1 < children.len() { y += gap.max(0); }
        }

        // Content size must be computed from the unscrolled layout. Scrolling
        // changes the viewport position of children; it must never change the
        // size of the content itself or the available scroll range.
        let natural_height = (y + padding.bottom).max(rect.height);
        let natural_width = children.iter()
            .map(|child| {
                result.rects.get(&child.id)
                    .map(|r| r.x + r.width + child.layout.margin.right)
                    .unwrap_or(0)
            })
            .max().unwrap_or(0)
            .max(rect.width);

        Size::new(natural_width.max(0) as u32, natural_height.max(0) as u32)
    }

    fn layout_row<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        _node: &TreeNode,
        rect: Rect,
        children: Vec<&TreeNode>,
        padding: EdgeInsets,
        gap: i32,
        align_items: Alignment,
        result: &mut LayoutResult,
        measurer: &M,
    ) -> Size {
        let content = inner_rect(rect, padding);
        if children.is_empty() {
            return Size::new(padding.horizontal().max(0) as u32, padding.vertical().max(0) as u32);
        }

        let gap_total = gap.max(0) * children.len().saturating_sub(1) as i32;
        let usable_width = (content.width - gap_total).max(0);
        let preferred_width = children.iter().map(|child| self.preferred_width(snapshot, child, measurer)).sum::<i32>();
        let fill_count = children.iter().filter(|child| matches!(child.layout.width, SizeMode::Fill)).count();
        let distributable = (usable_width - preferred_width).max(0);
        let share = if fill_count == 0 { 0 } else { distributable / fill_count as i32 };
        let remainder = if fill_count == 0 { 0 } else { distributable % fill_count as i32 };

        let mut x = content.x;
        let mut fill_index = 0;
        for (index, child) in children.iter().enumerate() {
            let margin = child.layout.margin;
            x += margin.left;
            let width = match child.layout.width {
                SizeMode::Fixed(value) => value.max(0),
                SizeMode::Auto => self.preferred_width(snapshot, child, measurer),
                SizeMode::Fill => {
                    let extra = if fill_index == 0 { remainder } else { 0 };
                    fill_index += 1;
                    (self.preferred_width(snapshot, child, measurer) + share + extra).max(0)
                }
            };

            let available_height = (content.height - margin.vertical()).max(0);
            let alignment = child.layout.align_self.unwrap_or(align_items);
            let height = match (alignment, child.layout.height) {
                (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available_height,
                (_, SizeMode::Auto) => self.preferred_height(snapshot, child, measurer, Some(width)).min(available_height),
                (_, mode) => resolve_width(mode, available_height),
            };
            let width = child.layout.constraints.clamp_width(width.max(0));
            let height = child.layout.constraints.clamp_height(height.max(0)).min(available_height.max(0));
            let y = aligned_start(content.y, margin.top, available_height, height, alignment);
            let child_rect = Rect::new(x, y, width, height);
            self.layout_node(snapshot, child, child_rect, result, measurer);

            x += width + margin.right;
            if index + 1 < children.len() { x += gap.max(0); }
        }

        // As with Column, measure the unscrolled content first. The scroll
        // offset is a viewport transform and must not feed back into the
        // intrinsic content size.
        let natural_width = (x + padding.right).max(rect.width);
        let natural_height = children.iter()
            .map(|child| {
                result.rects.get(&child.id)
                    .map(|r| r.y + r.height + child.layout.margin.bottom)
                    .unwrap_or(0)
            })
            .max().unwrap_or(0)
            .max(rect.height);

        Size::new(natural_width.max(0) as u32, natural_height.max(0) as u32)
    }

    fn preferred_width<M: IntrinsicMeasurer>(&self, snapshot: &TreeSnapshot, node: &TreeNode, measurer: &M) -> i32 {
        let base = match node.kind {
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {
                measurer.measure(node.kind, node.text.as_deref(), None).width as i32
            }
            NodeKind::Column | NodeKind::Row => self.container_preferred_width(snapshot, node, measurer),
        };
        node.layout.constraints.clamp_width(base + node.layout.margin.horizontal())
    }

    fn preferred_height<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
        max_width: Option<i32>,
    ) -> i32 {
        let height = self.preferred_content_height(snapshot, node, measurer, max_width)
            + node.layout.margin.vertical();
        node.layout.constraints.clamp_height(height)
    }

    fn preferred_content_height<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
        max_width: Option<i32>,
    ) -> i32 {
        match node.kind {
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {
                measurer.measure(node.kind, node.text.as_deref(), max_width).height as i32
            }
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                let children = ordered_children(snapshot, node.id);
                style.padding.vertical()
                    + children
                        .iter()
                        .map(|child| self.preferred_height(snapshot, child, measurer, max_width))
                        .sum::<i32>()
                    + style.gap.max(0) * children.len().saturating_sub(1) as i32
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                let children = ordered_children(snapshot, node.id);
                style.padding.vertical()
                    + children
                        .iter()
                        .map(|child| self.preferred_height(snapshot, child, measurer, preferred_width_hint(child).or(max_width)))
                        .max()
                        .unwrap_or(0)
            }
        }
    }

    fn container_preferred_width<M: IntrinsicMeasurer>(&self, snapshot: &TreeSnapshot, node: &TreeNode, measurer: &M) -> i32 {
        let children = ordered_children(snapshot, node.id);
        match node.kind {
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                style.padding.horizontal() + children.iter().map(|child| self.preferred_width(snapshot, child, measurer)).max().unwrap_or(0)
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                style.padding.horizontal()
                    + children.iter().map(|child| self.preferred_width(snapshot, child, measurer)).sum::<i32>()
                    + style.gap.max(0) * children.len().saturating_sub(1) as i32
            }
            _ => 0,
        }
    }
}

fn preferred_width_hint(node: &TreeNode) -> Option<i32> {
    match node.layout.width {
        SizeMode::Fixed(value) => Some(value.max(0)),
        SizeMode::Auto | SizeMode::Fill => node.layout.constraints.max_width,
    }
}

fn ordered_children<'a>(snapshot: &'a TreeSnapshot, parent: NodeId) -> Vec<&'a TreeNode> {
    let mut children = snapshot.nodes().filter(|candidate| candidate.parent == Some(parent)).collect::<Vec<_>>();
    children.sort_by_key(|child| child.index);
    children
}

fn inner_rect(rect: Rect, padding: EdgeInsets) -> Rect {
    Rect::new(
        rect.x + padding.left,
        rect.y + padding.top,
        (rect.width - padding.horizontal()).max(0),
        (rect.height - padding.vertical()).max(0),
    )
}

fn aligned_start(origin: i32, margin_start: i32, available: i32, size: i32, alignment: Alignment) -> i32 {
    let remaining = (available - size).max(0);
    origin + margin_start + match alignment {
        Alignment::Start | Alignment::Stretch => 0,
        Alignment::Center => remaining / 2,
        Alignment::End => remaining,
    }
}

fn resolve_width(mode: SizeMode, available: i32) -> i32 {
    match mode {
        SizeMode::Fixed(value) => value.max(0).min(available.max(0)),
        SizeMode::Auto | SizeMode::Fill => available.max(0),
    }
}

/// The framework's declarative UI tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Label(Label),
    Button(Button),
    TextInput(TextInput),
    Column(Column),
    Row(Row),
}

impl Node {
    pub fn label(key: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self::Label(Label::new(
            NodeId::from_key(key.as_ref()),
            text,
            LayoutStyle::default(),
        ))
    }

    pub fn label_with_layout(
        key: impl AsRef<str>,
        text: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::Label(Label::new(NodeId::from_key(key.as_ref()), text, layout))
    }

    pub fn button(key: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self::Button(Button::new(
            NodeId::from_key(key.as_ref()),
            text,
            LayoutStyle::default(),
        ))
    }

    pub fn button_with_layout(
        key: impl AsRef<str>,
        text: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::Button(Button::new(NodeId::from_key(key.as_ref()), text, layout))
    }

    pub fn text_input(key: impl AsRef<str>, value: impl Into<String>) -> Self {
        Self::TextInput(TextInput::new(
            NodeId::from_key(key.as_ref()),
            value,
            LayoutStyle::default(),
        ))
    }

    pub fn text_input_with_layout(
        key: impl AsRef<str>,
        value: impl Into<String>,
        layout: LayoutStyle,
    ) -> Self {
        Self::TextInput(TextInput::new(NodeId::from_key(key.as_ref()), value, layout))
    }

    pub fn column(key: impl AsRef<str>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::Column(Column::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            ColumnStyle::default(),
            LayoutStyle::default(),
        ))
    }

    pub fn column_with_layout(
        key: impl AsRef<str>,
        children: impl IntoIterator<Item = Node>,
        layout: LayoutStyle,
        style: ColumnStyle,
    ) -> Self {
        Self::Column(Column::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            style,
            layout,
        ))
    }

    pub fn row(key: impl AsRef<str>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::Row(Row::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            RowStyle::default(),
            LayoutStyle::default(),
        ))
    }

    pub fn row_with_layout(
        key: impl AsRef<str>,
        children: impl IntoIterator<Item = Node>,
        layout: LayoutStyle,
        style: RowStyle,
    ) -> Self {
        Self::Row(Row::new(
            NodeId::from_key(key.as_ref()),
            children.into_iter().collect(),
            style,
            layout,
        ))
    }

    pub fn with_accessibility(self, accessibility: AccessibilityInfo) -> Self {
        match self {
            Self::Label(mut node) => { node.accessibility = accessibility; Self::Label(node) }
            Self::Button(mut node) => { node.accessibility = accessibility; Self::Button(node) }
            Self::TextInput(mut node) => { node.accessibility = accessibility; Self::TextInput(node) }
            Self::Column(mut node) => { node.accessibility = accessibility; Self::Column(node) }
            Self::Row(mut node) => { node.accessibility = accessibility; Self::Row(node) }
        }
    }

    pub fn accessibility(&self) -> &AccessibilityInfo {
        match self {
            Self::Label(node) => node.accessibility(),
            Self::Button(node) => node.accessibility(),
            Self::TextInput(node) => node.accessibility(),
            Self::Column(node) => node.accessibility(),
            Self::Row(node) => node.accessibility(),
        }
    }

    pub fn id(&self) -> NodeId {
        match self {
            Self::Label(label) => label.id(),
            Self::Button(button) => button.id(),
            Self::TextInput(input) => input.id(),
            Self::Column(column) => column.id(),
            Self::Row(row) => row.id(),
        }
    }

    pub fn kind(&self) -> NodeKind {
        match self {
            Self::Label(_) => NodeKind::Label,
            Self::Button(_) => NodeKind::Button,
            Self::TextInput(_) => NodeKind::TextInput,
            Self::Column(_) => NodeKind::Column,
            Self::Row(_) => NodeKind::Row,
        }
    }

    pub fn layout(&self) -> LayoutStyle {
        match self {
            Self::Label(label) => label.layout(),
            Self::Button(button) => button.layout(),
            Self::TextInput(input) => input.layout(),
            Self::Column(column) => column.layout(),
            Self::Row(row) => row.layout(),
        }
    }

    pub fn column_style(&self) -> Option<ColumnStyle> {
        match self {
            Self::Column(column) => Some(column.style()),
            _ => None,
        }
    }

    pub fn row_style(&self) -> Option<RowStyle> {
        match self {
            Self::Row(row) => Some(row.style()),
            _ => None,
        }
    }

    pub fn visit(&self, visitor: &mut impl FnMut(&Node, Option<NodeId>, usize)) {
        self.visit_with_parent(None, 0, visitor);
    }

    pub fn contains_id(&self, target: NodeId) -> bool {
        let mut found = false;
        self.visit(&mut |node, _, _| {
            if node.id() == target {
                found = true;
            }
        });
        found
    }

    fn visit_with_parent(
        &self,
        parent: Option<NodeId>,
        index: usize,
        visitor: &mut impl FnMut(&Node, Option<NodeId>, usize),
    ) {
        visitor(self, parent, index);

        match self {
            Self::Column(column) => {
                for (index, child) in column.children().iter().enumerate() {
                    child.visit_with_parent(Some(column.id()), index, visitor);
                }
            }
            Self::Row(row) => {
                for (index, child) in row.children().iter().enumerate() {
                    child.visit_with_parent(Some(row.id()), index, visitor);
                }
            }
            _ => {}
        }
    }

    fn validate_unique_ids(&self) -> Result<(), TreeError> {
        let mut seen = HashSet::new();
        self.visit(&mut |node, _, _| {
            seen.insert(node.id());
        });

        let mut count = HashMap::new();
        self.visit(&mut |node, _, _| {
            *count.entry(node.id()).or_insert(0usize) += 1;
        });

        if let Some((id, _)) = count.into_iter().find(|(_, count)| *count > 1) {
            return Err(TreeError::DuplicateNodeId(id));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Label,
    Button,
    TextInput,
    Column,
    Row,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    id: NodeId,
    text: String,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
}

impl Label {
    fn new(id: NodeId, text: impl Into<String>, layout: LayoutStyle) -> Self {
        let text = text.into();
        let accessibility = AccessibilityInfo::new(AccessibilityRole::Label)
            .name(text.clone())
            .focusable(false);
        Self { id, text, layout, accessibility }
    }

    pub fn id(&self) -> NodeId { self.id }
    pub fn text(&self) -> &str { &self.text }
    pub fn layout(&self) -> LayoutStyle { self.layout }
    pub fn accessibility(&self) -> &AccessibilityInfo { &self.accessibility }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    id: NodeId,
    text: String,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
}

impl Button {
    fn new(id: NodeId, text: impl Into<String>, layout: LayoutStyle) -> Self {
        let text = text.into();
        let accessibility = AccessibilityInfo::new(AccessibilityRole::Button)
            .name(text.clone())
            .focusable(true);
        Self { id, text, layout, accessibility }
    }

    pub fn id(&self) -> NodeId { self.id }
    pub fn text(&self) -> &str { &self.text }
    pub fn layout(&self) -> LayoutStyle { self.layout }
    pub fn accessibility(&self) -> &AccessibilityInfo { &self.accessibility }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput {
    id: NodeId,
    value: String,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
}

impl TextInput {
    fn new(id: NodeId, value: impl Into<String>, layout: LayoutStyle) -> Self {
        Self {
            id,
            value: value.into(),
            layout,
            accessibility: AccessibilityInfo::new(AccessibilityRole::TextInput)
                .name("Text input")
                .focusable(true),
        }
    }

    pub fn id(&self) -> NodeId { self.id }
    pub fn value(&self) -> &str { &self.value }
    pub fn layout(&self) -> LayoutStyle { self.layout }
    pub fn accessibility(&self) -> &AccessibilityInfo { &self.accessibility }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    id: NodeId,
    children: Vec<Node>,
    style: ColumnStyle,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
}

impl Column {
    fn new(id: NodeId, children: Vec<Node>, style: ColumnStyle, layout: LayoutStyle) -> Self {
        Self { id, children, style, layout, accessibility: AccessibilityInfo::new(AccessibilityRole::Group).focusable(false) }
    }

    pub fn id(&self) -> NodeId { self.id }
    pub fn children(&self) -> &[Node] { &self.children }
    pub fn style(&self) -> ColumnStyle { self.style }
    pub fn layout(&self) -> LayoutStyle { self.layout }
    pub fn accessibility(&self) -> &AccessibilityInfo { &self.accessibility }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    id: NodeId,
    children: Vec<Node>,
    style: RowStyle,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
}

impl Row {
    fn new(id: NodeId, children: Vec<Node>, style: RowStyle, layout: LayoutStyle) -> Self {
        Self { id, children, style, layout, accessibility: AccessibilityInfo::new(AccessibilityRole::Group).focusable(false) }
    }

    pub fn id(&self) -> NodeId { self.id }
    pub fn children(&self) -> &[Node] { &self.children }
    pub fn style(&self) -> RowStyle { self.style }
    pub fn layout(&self) -> LayoutStyle { self.layout }
    pub fn accessibility(&self) -> &AccessibilityInfo { &self.accessibility }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub index: usize,
    pub text: Option<String>,
    pub layout: LayoutStyle,
    pub column_style: Option<ColumnStyle>,
    pub row_style: Option<RowStyle>,
    pub accessibility: AccessibilityInfo,
}

impl TreeNode {
    fn from_node(node: &Node, parent: Option<NodeId>, index: usize) -> Self {
        let text = match node {
            Node::Label(label) => Some(label.text().to_owned()),
            Node::Button(button) => Some(button.text().to_owned()),
            Node::TextInput(input) => Some(input.value().to_owned()),
            Node::Column(_) | Node::Row(_) => None,
        };

        Self {
            id: node.id(),
            kind: node.kind(),
            parent,
            index,
            text,
            layout: node.layout(),
            column_style: node.column_style(),
            row_style: node.row_style(),
            accessibility: node.accessibility().clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeSnapshot {
    nodes: HashMap<NodeId, TreeNode>,
}

impl TreeSnapshot {
    pub fn from_node(root: &Node) -> Result<Self, TreeError> {
        root.validate_unique_ids()?;

        let mut nodes = HashMap::new();
        root.visit(&mut |node, parent, index| {
            nodes.insert(node.id(), TreeNode::from_node(node, parent, index));
        });

        Ok(Self { nodes })
    }

    pub fn get(&self, id: NodeId) -> Option<&TreeNode> { self.nodes.get(&id) }
    pub fn contains(&self, id: NodeId) -> bool { self.nodes.contains_key(&id) }
    pub fn nodes(&self) -> impl Iterator<Item = &TreeNode> { self.nodes.values() }

    /// Returns nodes in declarative preorder, preserving sibling indices.
    pub fn ordered_nodes(&self) -> Vec<&TreeNode> {
        let mut ordered = Vec::with_capacity(self.nodes.len());
        let root = self.nodes.values().find(|node| node.parent.is_none()).map(|node| node.id);
        if let Some(root) = root {
            collect_ordered_nodes(self, root, &mut ordered);
        }
        ordered
    }
}

fn collect_ordered_nodes<'a>(snapshot: &'a TreeSnapshot, id: NodeId, output: &mut Vec<&'a TreeNode>) {
    let Some(node) = snapshot.get(id) else { return; };
    output.push(node);
    let mut children = snapshot
        .nodes()
        .filter(|candidate| candidate.parent == Some(id))
        .collect::<Vec<_>>();
    children.sort_by_key(|child| child.index);
    for child in children {
        collect_ordered_nodes(snapshot, child.id, output);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeOp {
    Insert(TreeNode),
    Update(TreeNode),
    Move {
        id: NodeId,
        parent: Option<NodeId>,
        index: usize,
    },
    Remove(TreeNode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDiff {
    pub operations: Vec<TreeOp>,
}

impl TreeDiff {
    pub fn between(previous: &TreeSnapshot, next: &TreeSnapshot) -> Self {
        let mut operations = Vec::new();

        // Remove deepest descendants first so platform backends can safely
        // dispose child objects before their logical parents.
        let mut removals = previous
            .nodes
            .values()
            .filter(|node| !next.contains(node.id))
            .cloned()
            .collect::<Vec<_>>();
        removals.sort_by_key(|node| std::cmp::Reverse(depth(previous, node.id)));
        operations.extend(removals.into_iter().map(TreeOp::Remove));

        // Insert shallow nodes before descendants so native parents can exist
        // before their children.
        let mut inserts = next
            .nodes
            .values()
            .filter(|node| !previous.contains(node.id))
            .cloned()
            .collect::<Vec<_>>();
        inserts.sort_by_key(|node| depth(next, node.id));
        operations.extend(inserts.into_iter().map(TreeOp::Insert));

        // Existing nodes are updated only when their semantic data changed;
        // order/parent changes are represented separately as Move operations.
        for node in next.nodes.values() {
            let Some(previous_node) = previous.get(node.id) else {
                continue;
            };

            if previous_node.kind != node.kind
                || previous_node.text != node.text
                || previous_node.layout != node.layout
                || previous_node.column_style != node.column_style
                || previous_node.row_style != node.row_style
                || previous_node.accessibility != node.accessibility {
                operations.push(TreeOp::Update(node.clone()));
            }

            if previous_node.parent != node.parent || previous_node.index != node.index {
                operations.push(TreeOp::Move {
                    id: node.id,
                    parent: node.parent,
                    index: node.index,
                });
            }
        }

        Self { operations }
    }
}

impl TreeDiff {
    pub fn invalidates_layout(&self) -> bool {
        self.operations.iter().any(|operation| matches!(
            operation,
            TreeOp::Insert(_) | TreeOp::Remove(_) | TreeOp::Move { .. } | TreeOp::Update(_)
        ))
    }
}

fn depth(snapshot: &TreeSnapshot, id: NodeId) -> usize {
    let mut depth = 0;
    let mut current = snapshot.get(id).and_then(|node| node.parent);

    while let Some(parent) = current {
        depth += 1;
        current = snapshot.get(parent).and_then(|node| node.parent);
    }

    depth
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    DuplicateNodeId(NodeId),
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNodeId(id) => write!(f, "duplicate UI node id: {}", id.get()),
        }
    }
}

impl Error for TreeError {}

pub trait Platform {
    type Error: Error + Send + Sync + 'static;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub struct UnsupportedPlatform;

impl fmt::Display for UnsupportedPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("this platform is not implemented by the selected backend")
    }
}

impl Error for UnsupportedPlatform {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn label(key: &str, text: &str) -> Node {
        Node::label(key, text)
    }

    #[derive(Clone, PartialEq)]
    struct ManagedChildProps {
        mounts: Rc<Cell<u32>>,
        unmounts: Rc<Cell<u32>>,
    }

    #[derive(Clone)]
    struct ManagedChild {
        count: u32,
        props: ManagedChildProps,
    }

    impl Component for ManagedChild {
        type Props = ManagedChildProps;
        type Message = ();

        fn new(props: Self::Props) -> Self {
            Self { count: 0, props }
        }

        fn props(&self) -> &Self::Props {
            &self.props
        }

        fn set_props(&mut self, props: Self::Props) {
            self.props = props;
        }

        fn view(&self) -> Node {
            Node::label("managed-child", format!("child count: {}", self.count))
        }

        fn update(&mut self, event: Event) {
            if let Event::Click { target } = event {
                if target == NodeId::from_key("managed-child") {
                    self.count += 1;
                }
            }
        }

        fn mounted(&mut self) {
            self.props.mounts.set(self.props.mounts.get() + 1);
        }

        fn unmounted(&mut self) {
            self.props.unmounts.set(self.props.unmounts.get() + 1);
        }
    }

    struct ManagedParent {
        show_child: bool,
        mounts: Rc<Cell<u32>>,
        unmounts: Rc<Cell<u32>>,
    }

    impl Component for ManagedParent {
        type Props = ();
        type Message = ();

        fn new(_: Self::Props) -> Self {
            Self {
                show_child: true,
                mounts: Rc::new(Cell::new(0)),
                unmounts: Rc::new(Cell::new(0)),
            }
        }

        fn props(&self) -> &Self::Props {
            static PROPS: () = ();
            &PROPS
        }

        fn set_props(&mut self, _: Self::Props) {}

        fn view(&self) -> Node {
            Node::label("managed-parent-placeholder", "managed")
        }

        fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
            let child = if self.show_child {
                Some(context.child_with_props(
                    "child",
                    ManagedChildProps {
                        mounts: self.mounts.clone(),
                        unmounts: self.unmounts.clone(),
                    },
                    ManagedChild::new,
                ))
            } else {
                None
            };

            Node::column(
                "managed-parent",
                [
                    Node::button("toggle-managed-child", "Toggle"),
                    child.unwrap_or_else(|| Node::label("no-child", "Child absent")),
                ],
            )
        }

        fn update(&mut self, event: Event) {
            if let Event::Click { target } = event {
                if target == NodeId::from_key("toggle-managed-child") {
                    self.show_child = !self.show_child;
                }
            }
        }
    }

    #[test]
    fn managed_component_tree_preserves_keyed_child_state() {
        let mounts = Rc::new(Cell::new(0));
        let unmounts = Rc::new(Cell::new(0));
        let mut tree = ComponentTree::new(ManagedParent {
            show_child: true,
            mounts: mounts.clone(),
            unmounts: unmounts.clone(),
        });

        assert_eq!(mounts.get(), 1);
        tree.dispatch(Event::Click { target: NodeId::from_key("managed-child") });
        let snapshot = TreeSnapshot::from_node(&tree.view()).unwrap();
        assert_eq!(
            snapshot
                .get(NodeId::from_key("managed-child"))
                .and_then(|node| node.text.as_deref()),
            Some("child count: 1")
        );
        assert_eq!(mounts.get(), 1);
        assert_eq!(unmounts.get(), 0);

        tree.dispatch(Event::Click { target: NodeId::from_key("toggle-managed-child") });
        assert_eq!(unmounts.get(), 1);

        tree.dispatch(Event::Click { target: NodeId::from_key("toggle-managed-child") });
        assert_eq!(mounts.get(), 2);
        assert_eq!(unmounts.get(), 1);
    }

    #[derive(Clone, PartialEq)]
    struct PropChildProps {
        title: String,
        prop_changes: Rc<Cell<u32>>,
        mounts: Rc<Cell<u32>>,
    }

    struct PropChild {
        props: PropChildProps,
        count: u32,
    }

    impl Component for PropChild {
        type Props = PropChildProps;
        type Message = ();

        fn new(props: Self::Props) -> Self {
            Self { props, count: 0 }
        }

        fn props(&self) -> &Self::Props {
            &self.props
        }

        fn set_props(&mut self, props: Self::Props) {
            self.props = props;
        }

        fn view(&self) -> Node {
            Node::column(
                "prop-child",
                [
                    Node::label("prop-title", self.props.title.clone()),
                    Node::label("prop-count", format!("count: {}", self.count)),
                ],
            )
        }

        fn update(&mut self, event: Event) {
            if let Event::Click { target } = event {
                if target == NodeId::from_key("prop-count") {
                    self.count += 1;
                }
            }
        }

        fn props_changed(&mut self) {
            self.props.prop_changes.set(self.props.prop_changes.get() + 1);
        }

        fn mounted(&mut self) {
            self.props.mounts.set(self.props.mounts.get() + 1);
        }
    }

    struct PropParent {
        title: String,
        prop_changes: Rc<Cell<u32>>,
        mounts: Rc<Cell<u32>>,
    }

    impl Component for PropParent {
        type Props = ();
        type Message = ();

        fn new(_: Self::Props) -> Self {
            Self {
                title: "First title".into(),
                prop_changes: Rc::new(Cell::new(0)),
                mounts: Rc::new(Cell::new(0)),
            }
        }

        fn props(&self) -> &Self::Props {
            static PROPS: () = ();
            &PROPS
        }

        fn set_props(&mut self, _: Self::Props) {}

        fn view(&self) -> Node {
            Node::label("prop-parent-placeholder", "props")
        }

        fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
            Node::column(
                "prop-parent",
                [
                    Node::button("change-props", "Change props"),
                    context.child_with_props(
                        "child",
                        PropChildProps {
                            title: self.title.clone(),
                            prop_changes: self.prop_changes.clone(),
                            mounts: self.mounts.clone(),
                        },
                        PropChild::new,
                    ),
                ],
            )
        }

        fn update(&mut self, event: Event) {
            if let Event::Click { target } = event {
                if target == NodeId::from_key("change-props") {
                    self.title = "Second title".into();
                }
            }
        }
    }

    #[test]
    fn changed_props_update_child_without_remounting_or_resetting_state() {
        let prop_changes = Rc::new(Cell::new(0));
        let mounts = Rc::new(Cell::new(0));
        let mut tree = ComponentTree::new(PropParent {
            title: "First title".into(),
            prop_changes: prop_changes.clone(),
            mounts: mounts.clone(),
        });

        assert_eq!(mounts.get(), 1);
        assert_eq!(prop_changes.get(), 0);

        let snapshot = TreeSnapshot::from_node(&tree.view()).unwrap();
        assert_eq!(snapshot.get(NodeId::from_key("prop-title")).unwrap().text.as_deref(), Some("First title"));

        tree.dispatch(Event::Click { target: NodeId::from_key("prop-count") });
        tree.dispatch(Event::Click { target: NodeId::from_key("change-props") });

        let snapshot = TreeSnapshot::from_node(&tree.view()).unwrap();
        assert_eq!(snapshot.get(NodeId::from_key("prop-title")).unwrap().text.as_deref(), Some("Second title"));
        assert_eq!(snapshot.get(NodeId::from_key("prop-count")).unwrap().text.as_deref(), Some("count: 1"));

        // The keyed child keeps its instance/state while receiving new props.
        assert_eq!(mounts.get(), 1);
        assert_eq!(prop_changes.get(), 1);
        assert_eq!(tree.components.values().filter(|entry| entry.component_type == std::any::TypeId::of::<PropChild>()).count(), 1);
    }

    #[test]
    fn child_callback_reaches_parent_without_remounting_child() {
        #[derive(Clone, PartialEq)]
        struct ChildProps {
            callback: Callback<ParentMessage>,
            mounts: Rc<Cell<u32>>,
        }

        struct Child {
            props: ChildProps,
            count: u32,
        }

        impl Component for Child {
            type Props = ChildProps;
            type Message = ();

            fn new(props: Self::Props) -> Self { Self { props, count: 0 } }
            fn props(&self) -> &Self::Props { &self.props }
            fn set_props(&mut self, props: Self::Props) { self.props = props; }
            fn view(&self) -> Node { Node::button("child-action", format!("Child {}", self.count)) }
            fn update(&mut self, event: Event) {
                if matches!(event, Event::Click { target } if target == NodeId::from_key("child-action")) {
                    self.count += 1;
                    self.props.callback.send(ParentMessage::ChildCount(self.count));
                }
            }
            fn mounted(&mut self) { self.props.mounts.set(self.props.mounts.get() + 1); }
        }

        #[derive(Clone, PartialEq, Debug)]
        enum ParentMessage { ChildCount(u32) }

        struct Parent {
            observed: u32,
            mounts: Rc<Cell<u32>>,
        }

        impl Component for Parent {
            type Props = ();
            type Message = ParentMessage;
            fn new(_: Self::Props) -> Self { Self { observed: 0, mounts: Rc::new(Cell::new(0)) } }
            fn props(&self) -> &Self::Props { static PROPS: () = (); &PROPS }
            fn set_props(&mut self, _: Self::Props) {}
            fn view(&self) -> Node { Node::label("parent-observed", format!("Observed {}", self.observed)) }
            fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
                let callback = context.callback();
                let child = context.child_with_props(
                    "child",
                    ChildProps { callback, mounts: self.mounts.clone() },
                    Child::new,
                );

                Node::column(
                    "parent-root",
                    [
                        Node::label("parent-observed", format!("Observed {}", self.observed)),
                        child,
                    ],
                )
            }
            fn update(&mut self, _event: Event) {}
            fn message(&mut self, message: Self::Message) {
                match message { ParentMessage::ChildCount(count) => self.observed = count }
            }
        }

        let mounts = Rc::new(Cell::new(0));
        let mut tree = ComponentTree::new(Parent { observed: 0, mounts: mounts.clone() });
        assert_eq!(mounts.get(), 1);
        tree.dispatch(Event::Click { target: NodeId::from_key("child-action") });
        let snapshot = TreeSnapshot::from_node(&tree.view()).unwrap();
        assert_eq!(snapshot.get(NodeId::from_key("parent-observed")).unwrap().text.as_deref(), Some("Observed 1"));
        assert_eq!(mounts.get(), 1);
    }

    #[test]
    fn callback_can_be_cloned_and_used_as_shared_channel() {
        let sink = Rc::new(RefCell::new(VecDeque::<QueuedMessage>::new()));
        let callback: Callback<u32> = Callback {
            target: ComponentId::ROOT,
            sink: sink.clone(),
            _marker: std::marker::PhantomData,
        };
        let second = callback.clone();
        callback.send(1);
        second.send(2);
        let values = sink.borrow_mut().drain(..).map(|queued| *queued.message.downcast::<u32>().unwrap()).collect::<Vec<_>>();
        assert_eq!(values, vec![1, 2]);
    }

    #[test]
    fn layout_respects_fixed_height_and_gap() {
        let root = Node::column_with_layout(
            "root",
            [
                Node::label_with_layout(
                    "a",
                    "A",
                    LayoutStyle::new().height(SizeMode::Fixed(20)),
                ),
                Node::label_with_layout(
                    "b",
                    "B",
                    LayoutStyle::new().height(SizeMode::Fixed(30)),
                ),
            ],
            LayoutStyle::new(),
            ColumnStyle::new()
                .padding(EdgeInsets::all(10))
                .gap(5),
        );

        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let layout = LayoutEngine::new().layout(&snapshot, Size::new(200, 100));

        assert_eq!(layout[&NodeId::from_key("a")], Rect::new(10, 10, 180, 20));
        assert_eq!(layout[&NodeId::from_key("b")], Rect::new(10, 35, 180, 30));
    }

    #[test]
    fn layout_supports_center_alignment_and_fixed_width() {
        let root = Node::column_with_layout(
            "root",
            [Node::label_with_layout(
                "a",
                "A",
                LayoutStyle::new()
                    .width(SizeMode::Fixed(40))
                    .height(SizeMode::Fixed(20)),
            )],
            LayoutStyle::new(),
            ColumnStyle::new().align_items(Alignment::Center),
        );

        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let layout = LayoutEngine::new().layout(&snapshot, Size::new(200, 100));

        assert_eq!(layout[&NodeId::from_key("a")], Rect::new(80, 24, 40, 20));
    }

    #[test]
    fn diff_updates_existing_node_without_inserting_it() {
        let old = TreeSnapshot::from_node(&Node::column("root", [label("a", "A")])).unwrap();
        let new = TreeSnapshot::from_node(&Node::column("root", [label("a", "B")])).unwrap();

        let diff = TreeDiff::between(&old, &new);

        assert_eq!(
            diff.operations,
            vec![TreeOp::Update(TreeNode {
                id: NodeId::from_key("a"),
                kind: NodeKind::Label,
                parent: Some(NodeId::from_key("root")),
                index: 0,
                text: Some("B".into()),
                layout: LayoutStyle::default(),
                column_style: None,
                row_style: None,
                accessibility: AccessibilityInfo::new(AccessibilityRole::Label).name("B").focusable(false),
            })]
        );
    }

    #[test]
    fn layout_is_deterministic_and_respects_child_order() {
        let root = Node::column(
            "root",
            [label("first", "First"), label("second", "Second")],
        );
        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let engine = LayoutEngine::default();
        let layout = engine.layout(&snapshot, Size::new(400, 200));

        assert!(layout[&NodeId::from_key("first")].y < layout[&NodeId::from_key("second")].y);
    }

    #[test]
    fn nested_layout_coordinates_are_relative_to_parent() {
        let root = Node::column(
            "root",
            [Node::column("nested", [label("child", "Child")])],
        );
        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let engine = LayoutEngine::default();
        let layout = engine.layout(&snapshot, Size::new(400, 300));

        assert_eq!(layout[&NodeId::from_key("nested")].x, 24);
        assert_eq!(layout[&NodeId::from_key("child")].x, 24);
        assert_eq!(layout[&NodeId::from_key("child")].y, 24);
    }

    #[test]
    fn row_lays_children_out_horizontally_using_intrinsic_widths() {
        let root = Node::row_with_layout(
            "root",
            [Node::label("a", "Hello"), Node::button("b", "Go")],
            LayoutStyle::new(),
            RowStyle::new().padding(EdgeInsets::all(10)).gap(8),
        );
        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let layout = LayoutEngine::new().layout(&snapshot, Size::new(300, 80));

        let a = layout[&NodeId::from_key("a")];
        let b = layout[&NodeId::from_key("b")];
        assert!(a.x < b.x);
        assert!(a.width > 0);
        assert!(b.width > 0);
        assert_eq!(a.y, b.y);
    }

    #[test]
    fn constraints_clamp_intrinsic_size() {
        let node = Node::label_with_layout(
            "label",
            "This is a long label",
            LayoutStyle::new().constraints(
                Constraints::new().min_width(100).max_width(120).min_height(20).max_height(40),
            ),
        );
        let snapshot = TreeSnapshot::from_node(&Node::column("root", [node])).unwrap();
        let layout = LayoutEngine::new().layout(&snapshot, Size::new(300, 100));
        let rect = layout.get(&NodeId::from_key("label")).unwrap();
        assert!((100..=120).contains(&rect.width));
        assert!((20..=40).contains(&rect.height));
    }

    #[test]
    fn bounded_measurement_can_produce_multiple_lines() {
        struct WrapMeasurer;
        impl IntrinsicMeasurer for WrapMeasurer {
            fn measure(&self, _kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
                let width = text.unwrap_or_default().chars().count() as i32 * 10;
                let line_width = max_width.unwrap_or(width.max(1)).max(1);
                let lines = ((width + line_width - 1) / line_width).max(1);
                Size::new(width.min(line_width) as u32, (lines * 20) as u32)
            }
        }

        let snapshot = TreeSnapshot::from_node(&Node::column(
            "root",
            [Node::label_with_layout("wrapped", "abcdefghij", LayoutStyle::new().width(SizeMode::Fixed(50)))],
        )).unwrap();
        let layout = LayoutEngine::new().layout_with(&snapshot, Size::new(200, 100), &WrapMeasurer);
        let rect = layout.get(&NodeId::from_key("wrapped")).unwrap();
        assert_eq!(rect.width, 50);
        assert_eq!(rect.height, 40);
    }

    #[test]
    fn text_update_invalidates_layout() {
        let old = TreeSnapshot::from_node(&Node::column("root", [Node::label("a", "A")])).unwrap();
        let new = TreeSnapshot::from_node(&Node::column("root", [Node::label("a", "A much longer label")])).unwrap();
        let diff = TreeDiff::between(&old, &new);
        assert!(diff.invalidates_layout());
    }

    #[test]
    fn diff_detects_insert_remove_and_move() {
        let old = TreeSnapshot::from_node(&Node::column(
            "root",
            [label("a", "A"), label("b", "B")],
        ))
        .unwrap();
        let new = TreeSnapshot::from_node(&Node::column(
            "root",
            [label("b", "B"), label("c", "C")],
        ))
        .unwrap();

        let diff = TreeDiff::between(&old, &new);

        assert!(diff.operations.contains(&TreeOp::Remove(TreeNode {
            id: NodeId::from_key("a"),
            kind: NodeKind::Label,
            parent: Some(NodeId::from_key("root")),
            index: 0,
            text: Some("A".into()),
            layout: LayoutStyle::default(),
            column_style: None,
            row_style: None,
            accessibility: AccessibilityInfo::new(AccessibilityRole::Label).name("A").focusable(false),
        })));
        assert!(diff.operations.contains(&TreeOp::Insert(TreeNode {
            id: NodeId::from_key("c"),
            kind: NodeKind::Label,
            parent: Some(NodeId::from_key("root")),
            index: 1,
            text: Some("C".into()),
            layout: LayoutStyle::default(),
            column_style: None,
            row_style: None,
            accessibility: AccessibilityInfo::new(AccessibilityRole::Label).name("C").focusable(false),
        })));
        assert!(diff.operations.contains(&TreeOp::Move {
            id: NodeId::from_key("b"),
            parent: Some(NodeId::from_key("root")),
            index: 0,
        }));
    }
    #[test]
    fn scrollable_column_reports_content_size_and_scroll_range() {
        let children = (0..10).map(|index| {
            Node::label_with_layout(
                format!("item-{index}"),
                format!("Item {index}"),
                LayoutStyle::new().height(SizeMode::Fixed(24)),
            )
        });
        let root = Node::column_with_layout(
            "root",
            children,
            LayoutStyle::new().height(SizeMode::Fixed(120)),
            ColumnStyle::new()
                .padding(EdgeInsets::all(8))
                .gap(4)
                .overflow(Overflow::Scroll),
        );

        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let mut offsets = HashMap::new();
        offsets.insert(NodeId::from_key("root"), Point::new(0, 30));
        let result = LayoutEngine::new().layout_result_with(
            &snapshot,
            Size::new(200, 120),
            &DefaultIntrinsicMeasurer,
            &offsets,
        );

        assert!(result.content_sizes[&NodeId::from_key("root")].height > 120);
        assert!(result.scroll_ranges[&NodeId::from_key("root")].height > 0);
        assert_eq!(result.rects[&NodeId::from_key("item-0")].y, 8);
        assert!(result.clips.contains_key(&NodeId::from_key("root")));

        let first_content_height = result.content_sizes[&NodeId::from_key("root")].height;
        let first_scroll_range = result.scroll_ranges[&NodeId::from_key("root")].height;

        offsets.insert(NodeId::from_key("root"), Point::new(0, 60));
        let scrolled = LayoutEngine::new().layout_result_with(
            &snapshot,
            Size::new(200, 120),
            &DefaultIntrinsicMeasurer,
            &offsets,
        );

        assert_eq!(scrolled.rects[&NodeId::from_key("item-0")].y, 8);
        assert_eq!(scrolled.content_sizes[&NodeId::from_key("root")].height, first_content_height);
        assert_eq!(scrolled.scroll_ranges[&NodeId::from_key("root")].height, first_scroll_range);
    }

    #[test]
    fn non_scrolling_layout_keeps_child_coordinates_unchanged() {
        let root = Node::column_with_layout(
            "root",
            [Node::label("child", "Child")],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(10)),
        );
        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let result = LayoutEngine::new().layout_result_with(
            &snapshot,
            Size::new(200, 100),
            &DefaultIntrinsicMeasurer,
            &HashMap::new(),
        );
        assert_eq!(result.rects[&NodeId::from_key("child")].x, 10);
        assert_eq!(result.rects[&NodeId::from_key("child")].y, 10);
    }

    #[test]
    fn text_input_is_controlled_and_focusable() {
        let node = Node::text_input("name", "Alice");
        assert_eq!(node.kind(), NodeKind::TextInput);
        assert_eq!(node.accessibility().role, AccessibilityRole::TextInput);
        assert!(node.accessibility().focusable);

        let snapshot = TreeSnapshot::from_node(&node).unwrap();
        let id = NodeId::from_key("name");
        assert_eq!(snapshot.get(id).and_then(|node| node.text.as_deref()), Some("Alice"));

        let event = Event::TextChanged {
            target: id,
            value: "Bob".into(),
        };
        assert_eq!(event.target(), Some(id));
    }

    #[derive(Debug)]
    struct LifecycleComponent {
        updates: u32,
        mounts: u32,
        unmounts: u32,
    }

    impl Component for LifecycleComponent {
        type Props = ();
        type Message = ();

        fn new(_: Self::Props) -> Self {
            panic!("LifecycleComponent is only constructed directly in this test")
        }

        fn props(&self) -> &Self::Props {
            static PROPS: () = ();
            &PROPS
        }

        fn set_props(&mut self, _: Self::Props) {}

        fn view(&self) -> Node {
            Node::label("child", format!("updates: {}", self.updates))
        }

        fn update(&mut self, _event: Event) {
            self.updates += 1;
        }

        fn mounted(&mut self) {
            self.mounts += 1;
        }

        fn unmounted(&mut self) {
            self.unmounts += 1;
        }
    }

    #[test]
    fn component_host_preserves_child_state_across_parent_renders() {
        let mut host = ComponentHost::new(LifecycleComponent {
            updates: 0,
            mounts: 0,
            unmounts: 0,
        });

        assert_eq!(host.component().mounts, 1);

        let event = Event::Click { target: NodeId::from_key("child") };
        assert!(host.update(event));
        assert_eq!(host.component().updates, 1);

        let first = host.view();
        let second = host.view();
        assert_eq!(first.id(), second.id());
        assert_eq!(host.component().updates, 1);
    }

    #[test]
    fn component_host_routes_only_events_owned_by_the_child() {
        let mut host = ComponentHost::new(LifecycleComponent {
            updates: 0,
            mounts: 0,
            unmounts: 0,
        });
        assert!(!host.update(Event::Click { target: NodeId::from_key("outside") }));
        assert_eq!(host.component().updates, 0);

        assert!(host.update(Event::Click { target: NodeId::from_key("child") }));
        assert_eq!(host.component().updates, 1);
    }

    #[test]
    fn component_host_lifecycle_is_stable_for_its_entire_lifetime() {
        let mut host = ComponentHost::new(LifecycleComponent {
            updates: 0,
            mounts: 0,
            unmounts: 0,
        });

        assert_eq!(host.component().mounts, 1);
        assert_eq!(host.component().unmounts, 0);

        let _ = host.view();
        let _ = host.view();
        host.update(Event::Click { target: NodeId::from_key("child") });

        assert_eq!(host.component().mounts, 1);
        assert_eq!(host.component().unmounts, 0);

        host.replace(LifecycleComponent {
            updates: 0,
            mounts: 0,
            unmounts: 0,
        });
        assert_eq!(host.component().mounts, 1);
        assert_eq!(host.component().unmounts, 0);
    }

    #[test]
    fn button_defaults_to_focusable_accessible_control() {
        let button = Node::button("save", "Save");
        assert_eq!(button.accessibility().role, AccessibilityRole::Button);
        assert_eq!(button.accessibility().name.as_deref(), Some("Save"));
        assert!(button.accessibility().focusable);
    }

    #[test]
    fn keyboard_events_report_optional_focus_target() {
        let event = Event::KeyDown {
            target: Some(NodeId::from_key("save")),
            key: KeyCode::Enter,
            modifiers: KeyModifiers::default(),
        };
        assert_eq!(event.target(), Some(NodeId::from_key("save")));
    }


    #[test]
    fn task_scope_cancels_all_owned_tasks_when_dropped() {
        use std::time::Duration;

        let scheduler = Scheduler::new();
        let scope = TaskScope::new(scheduler.clone(), ComponentId::ROOT);
        let first = scope.spawn(async {
            SleepFuture::new(Duration::from_millis(30)).await;
            1u32
        });
        let second = scope.spawn(async {
            SleepFuture::new(Duration::from_millis(30)).await;
            2u32
        });

        assert_eq!(scope.task_count(), 2);
        drop(scope);
        assert!(first.is_cancelled());
        assert!(second.is_cancelled());

        std::thread::sleep(Duration::from_millis(50));
        assert!(scheduler.drain().is_empty());
    }

    #[test]
    fn component_unmount_cancels_its_task_scope() {
        use std::time::Duration;

        #[derive(Clone, PartialEq, Default)]
        struct Props;

        enum Message { Done }

        struct Child {
            show: bool,
            task: Option<TaskHandle>,
        }

        impl Component for Child {
            type Props = Props;
            type Message = Message;

            fn new(_: Props) -> Self { Self { show: false, task: None } }
            fn props(&self) -> &Props { static PROPS: Props = Props; &PROPS }
            fn set_props(&mut self, _: Props) {}
            fn view(&self) -> Node { Node::label("child", if self.show { "done" } else { "idle" }) }
            fn update(&mut self, _: Event) {}
            fn message(&mut self, _: Message) { self.show = true; self.task = None; }

            fn render(&mut self, context: &mut ComponentContext<'_, Message>) -> Node {
                if self.task.is_none() && !self.show {
                    let task = context.task_scope().spawn(async move {
                        SleepFuture::new(Duration::from_millis(40)).await;
                        Message::Done
                    });
                    self.task = Some(task);
                }
                self.view()
            }
        }

        struct Parent { show: bool }
        impl Component for Parent {
            type Props = ();
            type Message = ();
            fn new(_: ()) -> Self { Self { show: true } }
            fn props(&self) -> &() { static PROPS: () = (); &PROPS }
            fn set_props(&mut self, _: ()) {}
            fn view(&self) -> Node { Node::label("parent", if self.show { "on" } else { "off" }) }
            fn update(&mut self, event: Event) {
                if event.target() == Some(NodeId::from_key("toggle")) { self.show = !self.show; }
            }
            fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
                if self.show { context.child::<Child>("child") } else { Node::label("empty", "empty") }
            }
        }

        let mut tree = ComponentTree::new(Parent::new(()));
        let child_id = ComponentId::child(ComponentId::ROOT, "child");
        assert!(tree.components.contains_key(&child_id));
        assert_eq!(tree.components.get(&child_id).unwrap().task_scope.task_count(), 1);

        // Render without the child. Its scope is cancelled as part of unmount.
        if let Some(root) = tree.components.get_mut(&ComponentId::ROOT) {
            root.component.update(Event::Click { target: NodeId::from_key("toggle") });
        }
        tree.render();
        assert!(!tree.components.contains_key(&child_id));

        std::thread::sleep(Duration::from_millis(60));
        assert!(tree.scheduler.drain().is_empty());
    }

    #[test]
    fn scheduler_delivers_async_message_to_component() {
        use std::time::Duration;

        #[derive(Clone, PartialEq)]
        struct AsyncProps;

        enum AsyncMessage { Done }

        struct AsyncComponent {
            completed: bool,
            task: Option<TaskHandle>,
        }

        impl Component for AsyncComponent {
            type Props = AsyncProps;
            type Message = AsyncMessage;

            fn new(_: Self::Props) -> Self { Self { completed: false, task: None } }
            fn props(&self) -> &Self::Props { static PROPS: AsyncProps = AsyncProps; &PROPS }
            fn set_props(&mut self, _: Self::Props) {}
            fn view(&self) -> Node { Node::label("status", if self.completed { "done" } else { "idle" }) }
            fn update(&mut self, _: Event) {}
            fn message(&mut self, message: Self::Message) {
                if matches!(message, AsyncMessage::Done) {
                    self.completed = true;
                    self.task = None;
                }
            }
            fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
                if self.task.is_none() && !self.completed {
                    let delay = context.sleep(Duration::from_millis(1));
                    self.task = Some(context.spawn(async move {
                        delay.await;
                        AsyncMessage::Done
                    }));
                }
                self.view()
            }
        }

        let mut tree = ComponentTree::new(AsyncComponent::new(AsyncProps));
        let mut completed = false;
        for _ in 0..200 {
            if tree.pump_tasks() {
                let mut text = None;
                tree.view().visit(&mut |node, _, _| {
                    if node.id() == NodeId::from_key("status") {
                        if let Node::Label(label) = node {
                            text = Some(label.text().to_owned());
                        }
                    }
                });
                completed = text.as_deref() == Some("done");
                if completed { break; }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(completed);
    }

}
