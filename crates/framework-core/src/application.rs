//! Multi-window orchestration: opening/closing windows, routing events and
//! task completions to the right window's [`ComponentTree`], and applying
//! deferred window-open/close requests queued by components (see
//! [`crate::component::WindowRequests`]).
//!
//! This is a distinct responsibility from [`crate::window`] (which owns
//! what a window *is* and its live state) and from [`crate::component`]
//! (which owns one window's tree in isolation): coordinating *several*
//! independent trees, each with their own scheduler and services handle, is
//! its own concern — see the standards audit's P1.21 finding on module
//! boundaries for why this split exists.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::component::{Component, ComponentTree, RenderError, WindowCommand};
use crate::event::Event;
use crate::identity::WindowId;
use crate::node::Node;
use crate::scheduler::Scheduler;
use crate::services::Services;
use crate::style::Theme;
use crate::window::{Window, WindowState, apply_window_event, event_window_id};

struct WindowEntry {
    window: Window,
    state: WindowState,
    components: ComponentTree,
}

/// Owns every open window's component tree and routes events, task
/// completions, and deferred window-open/close requests between them.
pub struct Application {
    windows: HashMap<WindowId, WindowEntry>,
    primary_window: WindowId,
    next_window_id: u64,
    services: Services,
    theme: Theme,
}

impl fmt::Debug for Application {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Application")
            .field("windows", &self.windows.len())
            .field("primary_window", &self.primary_window)
            .finish_non_exhaustive()
    }
}

impl Application {
    pub fn new<C: Component>(component: C, window: Window) -> Self {
        Self::with_services_and_theme(component, window, Services::default(), Theme::default())
    }

    pub fn with_services<C: Component>(component: C, window: Window, services: Services) -> Self {
        Self::with_services_and_theme(component, window, services, Theme::default())
    }

    pub fn with_services_and_theme<C: Component>(
        component: C,
        window: Window,
        services: Services,
        theme: Theme,
    ) -> Self {
        let primary_window = WindowId::PRIMARY;
        let entry = WindowEntry {
            state: WindowState::new(window.size()),
            window,
            components: ComponentTree::with_services_and_theme(
                component,
                services.clone(),
                theme.clone(),
            ),
        };
        let mut windows = HashMap::new();
        windows.insert(primary_window, entry);
        let mut application = Self { windows, primary_window, next_window_id: 1, services, theme };
        // A component may request another window from its first render. The
        // root tree is rendered while this Application is being
        // constructed, so drain those requests only after the primary
        // entry is installed.
        application.apply_queued_window_commands();
        application
    }

    pub fn dispatch(&mut self, event: Event) -> bool {
        self.dispatch_to_window(self.primary_window, event)
    }

    pub fn dispatch_to_window(&mut self, id: WindowId, event: Event) -> bool {
        if event_window_id(&event).is_some_and(|event_window| event_window != id) {
            return false;
        }

        let Some(entry) = self.windows.get_mut(&id) else {
            return false;
        };

        apply_window_event(&mut entry.state, &event);
        let handled = entry.components.dispatch(event);
        let commands = entry.components.take_window_commands();
        self.apply_window_commands(commands);
        handled
    }

    /// Applies deferred window-open/close requests queued by a component
    /// through `ComponentContext::windows()`. Applied after the requesting
    /// window's own dispatch/task-pump/render finishes so a component never
    /// mutates the window registry while it is itself mid-render.
    fn apply_window_commands(&mut self, commands: Vec<WindowCommand>) {
        for command in commands {
            match command {
                WindowCommand::Open(constructor) => {
                    constructor(self);
                }
                WindowCommand::Close(id) => {
                    self.close_window(id);
                }
            }
        }
    }

    /// Returns the primary window's current rendered tree.
    ///
    /// # Panics
    ///
    /// Panics if the primary window has been closed. `Application` does not
    /// currently expose a way to close the primary window itself (only
    /// secondary windows via `ComponentContext::windows`), so this cannot
    /// happen through the public API today.
    #[must_use]
    pub fn view(&self) -> Node {
        self.view_for(self.primary_window).expect("primary window must exist")
    }
    #[must_use]
    pub fn view_for(&self, id: WindowId) -> Option<Node> {
        self.windows.get(&id).map(|entry| entry.components.view())
    }

    /// Re-renders the primary window's tree explicitly (outside of an event
    /// or task-completion transaction).
    ///
    /// # Errors
    ///
    /// See [`ComponentTree::render`].
    pub fn render(&mut self) -> Result<(), RenderError> {
        self.render_window(self.primary_window)
    }

    /// # Errors
    ///
    /// See [`ComponentTree::render`]. Returns `Ok(())` if `id` does not
    /// name an open window (there is nothing to render, which is not
    /// itself an error).
    pub fn render_window(&mut self, id: WindowId) -> Result<(), RenderError> {
        let result = match self.windows.get_mut(&id) {
            Some(entry) => entry.components.render(),
            None => Ok(()),
        };
        // Explicit renders have the same deferred-command guarantee as an
        // event or task transaction. Without this, a request made during a
        // first/manual render would wait for an unrelated later event.
        self.apply_queued_window_commands();
        result
    }

    #[must_use]
    pub fn components(&self) -> &ComponentTree {
        &self.windows[&self.primary_window].components
    }
    #[must_use]
    pub fn components_for(&self, id: WindowId) -> Option<&ComponentTree> {
        self.windows.get(&id).map(|entry| &entry.components)
    }
    pub fn pump_tasks(&mut self) -> bool {
        let mut changed = false;
        for id in self.window_ids() {
            changed |= self.pump_tasks_for(id);
        }
        changed
    }
    /// Pumps completions for one native window without causing unrelated
    /// windows to rerender.
    pub fn pump_tasks_for(&mut self, id: WindowId) -> bool {
        let Some(entry) = self.windows.get_mut(&id) else {
            return false;
        };
        let changed = entry.components.pump_tasks();
        let commands = entry.components.take_window_commands();
        self.apply_window_commands(commands);
        changed
    }
    #[must_use]
    pub fn scheduler(&self) -> &Scheduler {
        self.components().scheduler()
    }
    #[must_use]
    pub fn scheduler_for(&self, id: WindowId) -> Option<&Scheduler> {
        self.windows.get(&id).map(|entry| entry.components.scheduler())
    }

    pub fn open_window<C: Component>(
        &mut self,
        component: C,
        window: Window,
        modal_parent: Option<WindowId>,
    ) -> WindowId {
        let id = self.allocate_window_id();
        self.windows.insert(
            id,
            WindowEntry {
                state: WindowState::new(window.size()).with_modal_parent(modal_parent),
                window,
                components: ComponentTree::with_services_and_theme(
                    component,
                    self.services.clone(),
                    self.theme.clone(),
                ),
            },
        );
        // The new root has already rendered and may itself have queued
        // follow-up requests. Apply them now so initial rendering is a
        // complete lifecycle transaction, including nested requests.
        self.apply_queued_window_commands();
        id
    }

    pub fn close_window(&mut self, id: WindowId) -> bool {
        if id == self.primary_window {
            return false;
        }
        if !self.windows.contains_key(&id) {
            return false;
        }

        // A modal child cannot remain alive after its parent disappears:
        // its native backend would otherwise retain a dangling modal
        // relationship and the application would report an impossible
        // window topology.
        let mut pending = vec![id];
        let mut closing = HashSet::new();
        while let Some(current) = pending.pop() {
            if !closing.insert(current) {
                continue;
            }
            pending.extend(self.windows.iter().filter_map(|(child, entry)| {
                (entry.state.modal_parent() == Some(current)).then_some(*child)
            }));
        }
        for window in closing {
            self.windows.remove(&window);
        }
        true
    }

    #[must_use]
    pub fn window_ids(&self) -> Vec<WindowId> {
        let mut ids = self.windows.keys().copied().collect::<Vec<_>>();
        ids.sort();
        ids
    }

    #[must_use]
    pub fn window_state(&self, id: WindowId) -> Option<&WindowState> {
        self.windows.get(&id).map(|entry| &entry.state)
    }
    pub fn window_state_mut(&mut self, id: WindowId) -> Option<&mut WindowState> {
        self.windows.get_mut(&id).map(|entry| &mut entry.state)
    }
    #[must_use]
    pub fn services(&self) -> &Services {
        &self.services
    }
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    #[must_use]
    pub fn window(&self) -> &Window {
        &self.windows[&self.primary_window].window
    }
    #[must_use]
    pub fn window_for(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id).map(|entry| &entry.window)
    }

    fn allocate_window_id(&mut self) -> WindowId {
        WindowId::next(&mut self.next_window_id)
    }

    fn apply_queued_window_commands(&mut self) {
        loop {
            let commands = self
                .window_ids()
                .into_iter()
                .flat_map(|id| {
                    self.windows
                        .get_mut(&id)
                        .map(|entry| entry.components.take_window_commands())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            if commands.is_empty() {
                break;
            }
            self.apply_window_commands(commands);
        }
    }
}
