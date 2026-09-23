//! [`HeadlessPlatform`]: the headless backend behind the ordinary
//! [`Platform`] seam.

use std::any::Any;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use framework_core::{
    Application, Capability, Platform, PlatformCapabilities, UnsupportedPlatform, WindowId,
};

use crate::measure::HeadlessMeasurer;
use crate::tree::HeadlessTree;

/// Runs an [`Application`] with no host: realizes every window into a
/// [`HeadlessTree`], pumps its tasks, and returns once the application has
/// been quiet for the idle timeout.
///
/// Use [`crate::HeadlessApp`] to *drive* an application; use this where
/// code is written against [`Platform`] and should run unchanged with no
/// display — a CI smoke test, a server rendering a tree to inspect it.
///
/// ```
/// use framework_core::{Application, Node, Platform, Size, Window, Component, Event};
/// use framework_headless::HeadlessPlatform;
///
/// struct Hello;
/// impl Component for Hello {
///     type Props = ();
///     type Message = ();
///     fn new((): ()) -> Self { Self }
///     fn props(&self) -> &() { &() }
///     fn set_props(&mut self, (): ()) {}
///     fn view(&self) -> Node { Node::label("hello", "Hello") }
///     fn update(&mut self, _event: Event) {}
/// }
///
/// let mut application = Application::new(Hello::new(()), Window::new("Hello", Size::new(200, 100)));
/// let mut platform = HeadlessPlatform::new();
/// platform.run(&mut application)?;
/// let tree = platform.realized(framework_core::WindowId::PRIMARY).expect("realized");
/// assert!(tree.describe().contains("\"Hello\""));
/// # Ok::<(), framework_core::UnsupportedPlatform>(())
/// ```
#[derive(Debug)]
pub struct HeadlessPlatform {
    idle_timeout: Duration,
    measurer: HeadlessMeasurer,
    trees: HashMap<WindowId, HeadlessTree>,
}

impl Default for HeadlessPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl HeadlessPlatform {
    /// A headless platform that returns after 100 ms without task activity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            idle_timeout: Duration::from_millis(100),
            measurer: HeadlessMeasurer::new(),
            trees: HashMap::new(),
        }
    }

    /// Returns after `timeout` without task activity instead.
    #[must_use]
    pub const fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// The realized model of window `id` after the last run.
    #[must_use]
    pub fn realized(&self, id: WindowId) -> Option<&HeadlessTree> {
        self.trees.get(&id)
    }

    fn realize(&mut self, application: &Application) {
        let ids = application.window_ids();
        self.trees.retain(|id, _| ids.contains(id));
        for id in ids {
            if let (Some(view), Some(state)) =
                (application.view_for(id), application.window_state(id))
            {
                self.trees.entry(id).or_default().realize(
                    &view,
                    application.theme(),
                    state.size(),
                    self.measurer,
                );
            }
        }
    }
}

impl Platform for HeadlessPlatform {
    type Error = UnsupportedPlatform;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error> {
        self.realize(application);
        let mut quiet_since = Instant::now();
        while quiet_since.elapsed() < self.idle_timeout {
            if application.pump_tasks() {
                self.realize(application);
                quiet_since = Instant::now();
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.realize(application);
        Ok(())
    }

    /// What this backend genuinely realizes in its model — and nothing it
    /// only records. Draw lists, for instance, are kept but never
    /// rasterized, so [`Capability::CustomDrawing`] is not claimed.
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::new([
            Capability::MultipleWindows,
            Capability::WindowManagement,
            Capability::StatePersistence,
            Capability::DeepLinks,
            Capability::Lifecycle,
        ])
    }

    fn native_extension(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_only_advertise_what_the_model_realizes() {
        let capabilities = HeadlessPlatform::new().capabilities();
        for unrealized in [
            Capability::CustomDrawing,
            Capability::NativeSurfaces,
            Capability::Clipboard,
            Capability::Menus,
            Capability::Animations,
            Capability::FileDialogs,
        ] {
            assert!(
                !capabilities.supports(unrealized),
                "{unrealized:?} is not realized headlessly"
            );
        }
    }
}
