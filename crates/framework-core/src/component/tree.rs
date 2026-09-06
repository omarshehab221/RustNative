//! The framework-managed, keyed component tree: creates/reuses child
//! components across renders, scopes their node identities (see
//! `crate::identity`), routes events and task results to the right
//! component, and commits reactive effects.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use super::Component;
use super::context::{ComponentContext, QueuedMessage, WindowCommand};
use super::effects::{DeclaredEffect, EffectContext, EffectDependencies, EffectEntry};
use super::error::RenderError;
use crate::event::Event;
use crate::identity::{ComponentId, NodeId};
use crate::node::Node;
use crate::scheduler::{CompletedTask, Scheduler, TaskScope};
use crate::services::Services;
use crate::style::Theme;

/// Object-safe facade over `Component`, letting `ComponentTree` store
/// heterogeneous component types behind one `Box<dyn ManagedComponent>` per
/// entry. Application code never implements this directly — it exists only
/// to erase `Component::Props`/`Component::Message` for tree bookkeeping.
trait ManagedComponent {
    fn render(
        &mut self,
        tree: &mut ComponentTree,
        id: ComponentId,
        generation: u64,
        task_scope: TaskScope,
    ) -> Node;
    fn update(&mut self, event: Event);
    fn message_any(&mut self, message: Box<dyn Any>) -> bool;
    fn props_equal(&self, props: &dyn Any) -> bool;
    fn set_props_any(&mut self, props: Box<dyn Any>) -> bool;
    fn props_changed(&mut self);
    fn updated(&mut self);
    fn unmounted(&mut self);
}

impl<C: Component> ManagedComponent for C {
    fn render(
        &mut self,
        tree: &mut ComponentTree,
        id: ComponentId,
        generation: u64,
        task_scope: TaskScope,
    ) -> Node {
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
        props.downcast_ref::<C::Props>().is_some_and(|incoming| self.props() == incoming)
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
    // Global native-tree ID -> component-local ID. This keeps component
    // event handlers independent of the composition path that realizes
    // their view.
    node_ids: HashMap<NodeId, NodeId>,
    used_generation: u64,
    task_scope: TaskScope,
    effects: HashMap<String, EffectEntry>,
}

/// Framework-owned keyed component tree.
///
/// Child components are created/reused during `render()`. A child that is
/// no longer requested by its parent is structurally removed and receives
/// exactly one `unmounted()` callback. Reappearing with the same key
/// creates a new instance, which receives a fresh `mounted()` callback.
pub struct ComponentTree {
    components: HashMap<ComponentId, ComponentEntry>,
    pending_children: HashMap<ComponentId, HashMap<String, ComponentId>>,
    node_owners: HashMap<NodeId, (ComponentId, NodeId)>,
    local_node_owners: HashMap<NodeId, Vec<(ComponentId, NodeId)>>,
    generation: u64,
    next_component_id: u64,
    root_view: Option<Node>,
    message_sink: Rc<RefCell<VecDeque<QueuedMessage>>>,
    window_commands: Rc<RefCell<VecDeque<WindowCommand>>>,
    scheduler: Scheduler,
    pending_effects: HashMap<ComponentId, Vec<DeclaredEffect>>,
    /// Structured, user-triggerable composition problems collected during
    /// the render pass currently in progress; drained (and the first one
    /// surfaced) at the end of [`Self::render`]. See
    /// [`RenderError`]'s own docs for why these do not panic.
    pending_render_errors: Vec<RenderError>,
    last_render_error: Option<RenderError>,
    services: Services,
    theme: Theme,
}

impl ComponentTree {
    /// Creates a tree rooted at `root`, with default services and theme.
    pub fn new<C: Component>(root: C) -> Self {
        Self::with_services(root, Services::default())
    }

    /// Creates a tree rooted at `root`, with `services` and the default
    /// theme.
    pub fn with_services<C: Component>(root: C, services: Services) -> Self {
        Self::with_services_and_theme(root, services, Theme::default())
    }

    /// Creates a tree rooted at `root`, with `services` and `theme`.
    pub fn with_services_and_theme<C: Component>(
        mut root: C,
        services: Services,
        theme: Theme,
    ) -> Self {
        root.mounted();
        let mut tree = Self {
            components: HashMap::new(),
            pending_children: HashMap::new(),
            node_owners: HashMap::new(),
            local_node_owners: HashMap::new(),
            generation: 0,
            next_component_id: 1,
            root_view: None,
            message_sink: Rc::new(RefCell::new(VecDeque::new())),
            window_commands: Rc::new(RefCell::new(VecDeque::new())),
            scheduler: Scheduler::new(),
            pending_effects: HashMap::new(),
            pending_render_errors: Vec::new(),
            last_render_error: None,
            services,
            theme,
        };
        let root_scope = TaskScope::new(tree.scheduler.clone(), ComponentId::ROOT);
        tree.components.insert(
            ComponentId::ROOT,
            ComponentEntry {
                component_type: std::any::TypeId::of::<C>(),
                component: Box::new(root),
                children: HashMap::new(),
                view: None,
                node_ids: HashMap::new(),
                used_generation: 0,
                task_scope: root_scope,
                effects: HashMap::new(),
            },
        );
        // The initial render's result is captured into `last_render_error`
        // (inspectable via `Self::last_render_error`) rather than
        // propagated as a `Result` from this constructor: every
        // `ComponentTree`/`Application` constructor already returns `Self`
        // directly, matching this crate's existing ergonomics, and a
        // render-time composition mistake should degrade gracefully rather
        // than make construction itself fallible.
        let _ = tree.render();
        tree
    }

    /// Re-renders the declarative tree from the root.
    ///
    /// # Panics
    ///
    /// Panics if this tree has produced more than `u64::MAX` renders over
    /// its lifetime (see `crate::identity`'s allocators for why this crate
    /// treats identity/generation-space exhaustion as an unrecoverable
    /// condition rather than something to silently wrap around).
    ///
    /// # Errors
    ///
    /// Returns the first [`RenderError`] detected during this render (for
    /// example, a component that used the same node key twice in one
    /// render). The tree still completes a structurally consistent render
    /// pass regardless — the offending duplicate is simply not registered a
    /// second time — so a caller can choose to log this and continue rather
    /// than lose the application to a panic; see [`Self::last_render_error`]
    /// to inspect the most recent outcome without re-triggering a render.
    #[allow(
        clippy::expect_used,
        reason = "the standards audit's P1.20 finding requires an identity allocator to fail loudly on \
    /// exhaustion rather than silently wrap and reuse a live id; there is no caller that \
    /// could act on a `Result` here"
    )]
    pub fn render(&mut self) -> Result<(), RenderError> {
        self.pending_render_errors.clear();
        self.generation =
            self.generation.checked_add(1).expect("framework render generation space exhausted");
        let generation = self.generation;
        let root = self.render_component(ComponentId::ROOT, generation);
        self.root_view = Some(root);
        self.rebuild_node_owners();
        self.commit_effects(generation);

        self.last_render_error = self.pending_render_errors.first().cloned();
        match &self.last_render_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// The first [`RenderError`] detected during the most recent
    /// [`Self::render`] call (via any path that triggers one — `dispatch`,
    /// `pump_tasks`, or an explicit `render` call), or `None` if that render
    /// completed without one.
    #[must_use]
    pub fn last_render_error(&self) -> Option<&RenderError> {
        self.last_render_error.as_ref()
    }

    /// Returns the current rendered tree.
    ///
    /// # Panics
    ///
    /// Panics if called before the first render. In practice this cannot
    /// happen through the public API: every constructor (`new`,
    /// `with_services`, `with_services_and_theme`) performs an initial
    /// render before returning.
    #[must_use]
    #[allow(
        clippy::expect_used,
        reason = "an invariant this runtime itself just established, not a condition an application can \
    /// trigger — see `crate::component::RenderError` for the line this crate draws between \
    /// the two"
    )]
    pub fn view(&self) -> Node {
        self.root_view.clone().expect("component tree must be rendered before its view is read")
    }

    /// Routes `event` to its owning component, re-rendering if it (or any
    /// message it caused to be sent) changed component state. Returns
    /// whether the event was delivered to a component.
    pub fn dispatch(&mut self, event: Event) -> bool {
        let (owner, event) = match event.target() {
            Some(target) => match self.node_owners.get(&target).copied().or_else(|| {
                self.local_node_owners
                    .get(&target)
                    .filter(|owners| owners.len() == 1)
                    .and_then(|owners| owners.first().copied())
            }) {
                Some((owner, local)) => (owner, event.with_local_target(Some(local))),
                // Targeted native events for removed objects are stale. They
                // must never be repurposed as root-component events.
                None => return false,
            },
            None => (ComponentId::ROOT, event),
        };

        let handled = self.update_component(owner, &event);

        // Child callbacks are transient runtime messages. They must be
        // delivered before the next declarative render so the render
        // observes the parent's updated state in the same event
        // transaction.
        let had_messages = !self.message_sink.borrow().is_empty();
        if had_messages {
            self.drain_messages();
        }

        if handled || had_messages {
            let _ = self.render();
        }

        handled || had_messages
    }

    /// Delivers every completed background-task result to its owning
    /// component, re-rendering if any changed state. Returns whether
    /// anything changed.
    pub fn pump_tasks(&mut self) -> bool {
        let completed = self.scheduler.drain();
        if completed.is_empty() {
            return false;
        }
        let mut changed = false;
        for completed_task in completed {
            let CompletedTask { target, message, .. } = completed_task;
            let Some(mut entry) = self.components.remove(&target) else {
                continue;
            };
            if entry.component.message_any(message) {
                entry.component.updated();
                changed = true;
            } else {
                // See `drain_messages` below for why this is not a normal,
                // ignorable outcome.
                debug_assert!(
                    false,
                    "task result for {target:?} did not match its own component's Message type; \
                     this indicates a framework bug in component identity/keying, not a \
                     legitimate runtime condition"
                );
                eprintln!(
                    "framework-core: dropped a task result for {target:?} because it did not \
                     match the target component's Message type (framework bug, not application \
                     code)"
                );
            }
            self.components.insert(target, entry);
        }
        if changed {
            let _ = self.render();
        }
        changed
    }

    /// Drains any window-open/close requests queued through
    /// `ComponentContext::windows()` during the most recent dispatch, task
    /// pump, or render. Called by `Application`, which owns the window
    /// registry these requests act on.
    pub(crate) fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        self.window_commands.borrow_mut().drain(..).collect()
    }

    pub(crate) fn message_sink(&self) -> &Rc<RefCell<VecDeque<QueuedMessage>>> {
        &self.message_sink
    }
    pub(crate) fn window_commands(&self) -> &Rc<RefCell<VecDeque<WindowCommand>>> {
        &self.window_commands
    }

    /// Returns the tree's scheduler.
    #[must_use]
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Returns the tree's platform-independent service contracts.
    #[must_use]
    pub fn services(&self) -> &Services {
        &self.services
    }
    /// Returns the tree's active theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub(crate) fn register_effect(
        &mut self,
        component: ComponentId,
        key: String,
        dependencies: EffectDependencies,
        run: Box<dyn FnOnce(EffectContext) -> super::effects::EffectCleanup>,
    ) {
        let effects = self.pending_effects.entry(component).or_default();
        if effects.iter().any(|effect| effect.key == key) {
            // A component composition mistake, not a framework-internal
            // impossible state (see `RenderError`'s docs): surface it
            // through the render result instead of panicking. The
            // second/duplicate registration for this key is dropped; the
            // first one already declared for this render stands.
            self.pending_render_errors.push(RenderError::DuplicateEffectKey { component, key });
            return;
        }
        effects.push(DeclaredEffect { key, dependencies, run });
    }

    fn drain_messages(&mut self) {
        loop {
            let next = self.message_sink.borrow_mut().pop_front();
            let Some(QueuedMessage { target, message }) = next else {
                break;
            };

            let Some(mut entry) = self.components.remove(&target) else {
                continue;
            };
            if entry.component.message_any(message) {
                entry.component.updated();
            } else {
                // `message_any` returns `false` only when the boxed
                // `Message` failed to downcast to the target component's
                // own `C::Message` — every message queued through
                // `ComponentContext<C::Message>` is constructed with that
                // exact type, so this is only reachable if a component's
                // identity/keying is broken (e.g. two different component
                // types ended up sharing a `ComponentId`), not a normal
                // "message for someone else" situation. Silently dropping
                // it here is exactly the "my button's onClick just...
                // didn't arrive" trap the standards audit calls out (P1.4):
                // surface it loudly instead of swallowing it.
                debug_assert!(
                    false,
                    "message for {target:?} did not match its own component's Message type; \
                     this indicates a framework bug in component identity/keying, not a \
                     legitimate runtime condition"
                );
                eprintln!(
                    "framework-core: dropped a message for {target:?} because it did not match \
                     the target component's Message type (framework bug, not application code)"
                );
            }
            self.components.insert(target, entry);
        }
    }

    /// Composes/reuses a keyed child component and returns its rendered
    /// view. `key` is consumed as an owned `String` (rather than `&str`)
    /// because it is stored as-is in `pending_children`'s key set below on
    /// every call path (create *and* reuse) — accepting a borrow would just
    /// move the same allocation to a `.to_owned()` at the call site with no
    /// net reduction, since `ComponentContext::child_with_props` already
    /// produces an owned `String` via `key.into()` before this is reached.
    #[allow(clippy::needless_pass_by_value)]
    #[allow(
        clippy::expect_used,
        reason = "an invariant this runtime itself just \n    /// established, not a condition an application can trigger — see \n    /// `crate::component::RenderError` for the line this crate draws between the two"
    )]
    pub(crate) fn render_child_with_props<C, F>(
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
        let existing_id =
            self.pending_children.get(&parent).and_then(|children| children.get(&key).copied());

        let needs_create = match existing_id {
            Some(existing_id) => self
                .components
                .get(&existing_id)
                .is_none_or(|entry| entry.component_type != std::any::TypeId::of::<C>()),
            None => true,
        };

        let id = existing_id.unwrap_or_else(|| self.allocate_component_id());

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
                    node_ids: HashMap::new(),
                    used_generation: generation,
                    task_scope: TaskScope::new(self.scheduler.clone(), id),
                    effects: HashMap::new(),
                },
            );
            self.pending_children.entry(parent).or_default().insert(key.clone(), id);
        } else {
            let changed = self
                .components
                .get(&id)
                .is_some_and(|entry| !entry.component.props_equal(&props as &dyn Any));

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

            self.pending_children.entry(parent).or_default().insert(key.clone(), id);
        }

        self.render_component(id, generation)
    }

    #[allow(
        clippy::expect_used,
        reason = "an invariant this runtime itself just established, not a condition an application can \
    /// trigger — see `crate::component::RenderError` for the line this crate draws between \
    /// the two"
    )]
    fn render_component(&mut self, id: ComponentId, generation: u64) -> Node {
        let mut entry =
            self.components.remove(&id).expect("component tree entry must exist while rendering");

        entry.used_generation = generation;
        let task_scope = entry.task_scope.clone();
        self.pending_children.insert(id, entry.children.clone());
        self.pending_effects.insert(id, Vec::new());
        let mut node = entry.component.render(self, id, generation, task_scope);
        let child_node_ids = self
            .pending_children
            .get(&id)
            .into_iter()
            .flat_map(|children| children.values())
            .filter_map(|child| self.components.get(child))
            .filter_map(|child| child.view.as_ref())
            .flat_map(|view| {
                let mut ids = Vec::new();
                view.visit(&mut |node, _, _| ids.push(node.id()));
                ids
            })
            .collect::<HashSet<_>>();
        let mut node_ids = HashMap::new();
        scope_component_node_ids(
            id,
            &mut node,
            &child_node_ids,
            &mut node_ids,
            &mut self.pending_render_errors,
        );
        entry.view = Some(node.clone());
        entry.node_ids = node_ids;
        entry.children = self.pending_children.remove(&id).unwrap_or_default();
        self.components.insert(id, entry);

        self.prune_unused_children(id, generation);
        node
    }

    fn commit_effects(&mut self, generation: u64) {
        let mut components = self
            .components
            .iter()
            .filter_map(|(id, entry)| (entry.used_generation == generation).then_some(*id))
            .collect::<Vec<_>>();
        components.sort();
        for id in components {
            self.commit_component_effects(id);
        }
    }

    fn commit_component_effects(&mut self, id: ComponentId) {
        let declarations = self.pending_effects.remove(&id).unwrap_or_default();
        let Some(mut entry) = self.components.remove(&id) else {
            return;
        };
        let mut declared = declarations
            .into_iter()
            .map(|effect| (effect.key.clone(), effect))
            .collect::<HashMap<_, _>>();

        let obsolete = entry
            .effects
            .keys()
            .filter(|key| !declared.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>();
        for key in obsolete {
            if let Some(mut effect) = entry.effects.remove(&key) {
                Self::dispose_effect(&mut effect);
            }
        }

        for (key, declaration) in declared.drain() {
            let should_restart = entry
                .effects
                .get(&key)
                .is_none_or(|current| !current.dependencies.equals(&declaration.dependencies));
            if !should_restart {
                continue;
            }
            if let Some(mut previous) = entry.effects.remove(&key) {
                Self::dispose_effect(&mut previous);
            }
            let scope = TaskScope::new(self.scheduler.clone(), id);
            let cleanup = (declaration.run)(EffectContext { task_scope: scope.clone() });
            entry.effects.insert(
                key,
                EffectEntry {
                    dependencies: declaration.dependencies,
                    task_scope: scope,
                    cleanup: Some(cleanup),
                },
            );
        }
        self.components.insert(id, entry);
    }

    fn dispose_effect(effect: &mut EffectEntry) {
        if let Some(cleanup) = effect.cleanup.take() {
            cleanup();
        }
        effect.task_scope.cancel_all();
    }

    fn dispose_effects(entry: &mut ComponentEntry) {
        for effect in entry.effects.values_mut() {
            Self::dispose_effect(effect);
        }
        entry.effects.clear();
    }

    fn rebuild_node_owners(&mut self) {
        self.node_owners.clear();
        self.local_node_owners.clear();
        for (component, entry) in &self.components {
            for (global, local) in &entry.node_ids {
                let previous = self.node_owners.insert(*global, (*component, *local));
                // Not a `RenderError`: given the collision-free identity
                // model in `crate::identity` and the duplicate-key check
                // already performed per-component in
                // `scope_component_node_ids`, two different components can
                // structurally never produce the same `global` id here.
                // Reaching this would be a framework bug, not a
                // user-triggerable composition mistake — see
                // `RenderError`'s docs for that distinction.
                debug_assert!(
                    previous.is_none(),
                    "duplicate node identity after component composition: {global:?}"
                );
                self.local_node_owners.entry(*local).or_default().push((*component, *local));
            }
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
                            .is_some_and(|child| child.used_generation != generation)
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

        // Structured task ownership ends with the component lifetime.
        // Cancel before unmounting so no task can legitimately outlive its
        // owner.
        entry.task_scope.cancel_all();
        Self::dispose_effects(&mut entry);

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

    fn allocate_component_id(&mut self) -> ComponentId {
        ComponentId::next(&mut self.next_component_id)
    }
}

impl Drop for ComponentTree {
    fn drop(&mut self) {
        if self.components.contains_key(&ComponentId::ROOT) {
            self.remove_component_children(ComponentId::ROOT);
            if let Some(mut root) = self.components.remove(&ComponentId::ROOT) {
                root.task_scope.cancel_all();
                Self::dispose_effects(&mut root);
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
                entry.task_scope.cancel_all();
                Self::dispose_effects(&mut entry);
                entry.component.unmounted();
            }
        }
    }
}

/// Rewrites every node an authored view directly contributed (i.e., every
/// node id *not* already present in `child_node_ids`, meaning it came from a
/// nested managed child's already-scoped view — see
/// `crate::component::context::ComponentContext::child`) from its
/// declarative local key into the framework-wide identity a platform
/// backend and `crate::reconcile` actually use, by combining it with
/// `owner` (see `crate::identity::NodeId::scoped`).
///
/// Detects — without panicking — the one composition mistake this step can
/// discover: the same component authoring the same local key more than once
/// in a single render. That is reported as
/// [`RenderError::DuplicateNodeKey`] rather than the `debug_assert!` this
/// function used before this rewrite (standards audit P1.10); the second
/// occurrence's scoped id is still assigned, so the tree remains
/// structurally well-formed even though the application has a bug to fix.
fn scope_component_node_ids(
    owner: ComponentId,
    node: &mut Node,
    child_node_ids: &HashSet<NodeId>,
    node_ids: &mut HashMap<NodeId, NodeId>,
    errors: &mut Vec<RenderError>,
) {
    let local = node.id();
    if child_node_ids.contains(&local) {
        return;
    }

    // Local keys are exposed back to `Component::update`, while the
    // platform sees an opaque component-scoped identity. This keeps
    // reusable components composable: two children can each have a
    // `"submit"` node — see `crate::identity` for the full identity model.
    let global = if owner == ComponentId::ROOT { local } else { NodeId::scoped(owner, local) };
    node.set_id(global);
    if node_ids.insert(global, local).is_some() {
        errors.push(RenderError::DuplicateNodeKey { component: owner });
    }

    match node {
        Node::Column(column) => {
            for child in column.children_mut() {
                scope_component_node_ids(owner, child, child_node_ids, node_ids, errors);
            }
        }
        Node::Row(row) => {
            for child in row.children_mut() {
                scope_component_node_ids(owner, child, child_node_ids, node_ids, errors);
            }
        }
        Node::Label(_) | Node::Button(_) | Node::TextInput(_) => {}
    }
}

#[cfg(test)]
mod tests;
