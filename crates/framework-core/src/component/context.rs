//! Render-time capabilities handed to a component: composing children,
//! spawning tasks, sending typed messages to a parent, and requesting a
//! sibling window be opened or closed.

use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use super::Component;
use super::effects::{EffectCleanup, EffectContext};
use super::tree::ComponentTree;
use crate::identity::{ComponentId, WindowId};
use crate::node::Node;
use crate::scheduler::{SleepFuture, TaskHandle, TaskScope};
use crate::services::Services;
use crate::style::Theme;
use crate::window::Window;

pub(crate) struct QueuedMessage {
    pub(crate) target: ComponentId,
    pub(crate) message: Box<dyn Any>,
}

/// A deferred request to open or close a sibling window, queued by a
/// component through [`ComponentContext::windows`] and applied by
/// [`crate::application::Application`] right after the requesting window's
/// dispatch/task-pump finishes. Opening is boxed as a constructor so
/// [`WindowRequests`] stays generic-free while still being able to build a
/// strongly typed `ComponentTree` once it reaches the `Application` that
/// owns the window registry.
pub(crate) enum WindowCommand {
    Open(Box<dyn FnOnce(&mut crate::application::Application) -> WindowId>),
    Close(WindowId),
}

/// A render-time capability for requesting that a sibling window be opened
/// or closed. Unlike a UI event, the request is deferred: it is queued here
/// and applied by the owning `Application` once the current dispatch or task
/// pump finishes, so opening a window never happens in the middle of
/// rendering the requesting window's own tree.
#[derive(Clone)]
pub struct WindowRequests {
    sink: Rc<RefCell<VecDeque<WindowCommand>>>,
}

impl WindowRequests {
    pub(crate) fn new(sink: Rc<RefCell<VecDeque<WindowCommand>>>) -> Self {
        Self { sink }
    }

    /// Requests that `window`, owned by a fresh `component`, be opened. When
    /// `modal_parent` is `Some`, the platform backend disables that window's
    /// native input while this one remains open.
    pub fn open<C: Component>(&self, component: C, window: Window, modal_parent: Option<WindowId>) {
        self.sink.borrow_mut().push_back(WindowCommand::Open(Box::new(
            move |application: &mut crate::application::Application| {
                application.open_window(component, window, modal_parent)
            },
        )));
    }

    /// Requests that `id` be closed. Requesting the primary window's own id
    /// is a harmless no-op, matching `Application::close_window`.
    pub fn close(&self, id: WindowId) {
        self.sink.borrow_mut().push_back(WindowCommand::Close(id));
    }
}

/// A typed child-to-parent message sender. Clones are cheap and represent
/// the same framework-managed channel endpoint.
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
        f.debug_struct("Callback").field("target", &self.target).finish_non_exhaustive()
    }
}

impl<M: 'static> Callback<M> {
    pub(crate) fn new(target: ComponentId, sink: Rc<RefCell<VecDeque<QueuedMessage>>>) -> Self {
        Self { target, sink, _marker: std::marker::PhantomData }
    }

    /// Queues `message` for delivery to the target component's
    /// [`Component::message`](crate::Component::message).
    pub fn send(&self, message: M) {
        self.sink
            .borrow_mut()
            .push_back(QueuedMessage { target: self.target, message: Box::new(message) });
    }
}

/// Render-time capabilities available to one component: composing children,
/// spawning tasks, requesting a re-render dependency (`effect`), sending
/// messages to a parent, and reaching platform services.
///
/// # Example
///
/// A component that composes a child and does asynchronous work. Both go
/// through the context: children so the framework can own their state
/// across renders, and tasks so they are cancelled when this component is
/// unmounted.
///
/// ```
/// use std::time::Duration;
///
/// use framework_core::{Component, ComponentContext, ComponentTree, Event, Node};
///
/// # #[derive(Clone, PartialEq, Default)]
/// # struct Unit;
/// # struct Child { props: Unit }
/// # impl Component for Child {
/// #     type Props = Unit;
/// #     type Message = ();
/// #     fn new(props: Self::Props) -> Self { Self { props } }
/// #     fn props(&self) -> &Self::Props { &self.props }
/// #     fn set_props(&mut self, props: Self::Props) { self.props = props; }
/// #     fn view(&self) -> Node { Node::label("child-label", "child") }
/// #     fn update(&mut self, _event: Event) {}
/// # }
/// struct Parent {
///     loaded: bool,
/// }
///
/// impl Component for Parent {
///     type Props = ();
///     type Message = &'static str;
///
///     fn new((): Self::Props) -> Self {
///         Self { loaded: false }
///     }
///     fn props(&self) -> &Self::Props {
///         &()
///     }
///     fn set_props(&mut self, (): Self::Props) {}
///
///     fn view(&self) -> Node {
///         Node::label("status", if self.loaded { "loaded" } else { "loading" })
///     }
///     fn update(&mut self, _event: Event) {}
///
///     // A task's output *is* the message, delivered here once the
///     // scheduler wakes the platform's event loop.
///     fn message(&mut self, _message: Self::Message) {
///         self.loaded = true;
///     }
///
///     fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
///         // An effect runs after this render commits, and re-runs only
///         // when its dependencies change — `()` here means "once".
///         context.effect("load-once", (), |effects| {
///             effects.spawn(async { "loaded" });
///             Box::new(|| { /* cleanup runs before the effect re-runs */ })
///         });
///
///         Node::column(
///             "root",
///             [
///                 self.view(),
///                 // The child's state is owned by the framework and
///                 // survives this component re-rendering.
///                 context.child::<Child>("child"),
///             ],
///         )
///     }
/// }
///
/// let mut tree = ComponentTree::new(Parent::new(()));
/// let Node::Column(root) = tree.view() else { panic!("the root is a column") };
/// assert_eq!(root.children().len(), 2);
/// ```
pub struct ComponentContext<'a, M: Send + 'static> {
    pub(super) tree: &'a mut ComponentTree,
    pub(super) parent: ComponentId,
    pub(super) generation: u64,
    pub(super) task_scope: TaskScope,
    pub(super) _message: std::marker::PhantomData<fn(M)>,
}

impl<M: Send + 'static> ComponentContext<'_, M> {
    /// Creates a typed child-to-parent callback for this component.
    #[must_use]
    pub fn callback<Msg: 'static>(&self) -> Callback<Msg> {
        Callback::new(self.parent, Rc::clone(self.tree.message_sink()))
    }

    /// Spawns `future` on this component's task scope, delivering its
    /// output as a message via [`Component::message`](crate::Component::message)
    /// when it completes.
    pub fn spawn<F>(&self, future: F) -> TaskHandle
    where
        F: std::future::Future<Output = M> + Send + 'static,
    {
        self.task_scope().spawn(future)
    }

    /// Returns the structured task scope owned by this component.
    ///
    /// The scope remains owned by the component even when this render-time
    /// handle is dropped. Tasks therefore survive renders but are
    /// automatically cancelled when the component leaves the managed tree.
    #[must_use]
    pub fn task_scope(&self) -> TaskScope {
        self.task_scope.clone()
    }

    /// Returns a future that completes after `duration`, driven by the
    /// scheduler rather than a real-time OS sleep (see
    /// [`crate::scheduler::Scheduler::sleep`]).
    #[must_use]
    pub fn sleep(&self, duration: Duration) -> SleepFuture {
        self.task_scope.scheduler().sleep(duration)
    }

    /// Returns the application-owned, platform-independent service
    /// contracts.
    #[must_use]
    pub fn services(&self) -> &Services {
        self.tree.services()
    }

    /// Returns a handle for requesting that a sibling window be opened or
    /// closed. Requests are deferred and applied once this window's current
    /// dispatch or task pump finishes.
    #[must_use]
    pub fn windows(&self) -> WindowRequests {
        WindowRequests::new(Rc::clone(self.tree.window_commands()))
    }

    /// Returns the active theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        self.tree.theme()
    }

    /// Registers work that follows this component's reactive dependencies.
    ///
    /// The effect runs only after the current declarative render is
    /// committed. On a later render it is retained while `dependencies` is
    /// unchanged; when they change, its cleanup runs and its effect-owned
    /// tasks are cancelled before the replacement effect starts. Removing
    /// the component follows the same cleanup and cancellation path.
    ///
    /// Calling this with a `key` already used earlier in the *same* render
    /// is a component composition mistake (see
    /// [`crate::component::RenderError::DuplicateEffectKey`]): the second
    /// registration is ignored and the mistake is surfaced through
    /// [`ComponentTree::last_render_error`] rather than through a panic.
    pub fn effect<D, F>(&mut self, key: impl Into<String>, dependencies: D, effect: F)
    where
        D: PartialEq + 'static,
        F: FnOnce(EffectContext) -> EffectCleanup + 'static,
    {
        self.tree.register_effect(
            self.parent,
            key.into(),
            super::effects::EffectDependencies::new(dependencies),
            Box::new(effect),
        );
    }

    /// Compose a child component with no meaningful inputs. Its props type
    /// must implement `Default`.
    pub fn child<C>(&mut self, key: impl Into<String>) -> Node
    where
        C: Component,
        C::Props: Default,
    {
        self.child_with_props(key, C::Props::default(), C::new)
    }

    /// Compose a child component with typed parent-provided inputs. The
    /// child instance is reused by key; changed props update the existing
    /// instance instead of remounting it.
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
        self.tree.render_child_with_props::<C, F>(
            self.parent,
            key.into(),
            props,
            self.generation,
            constructor,
        )
    }
}
