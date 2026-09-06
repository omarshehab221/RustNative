//! Platform-independent primitives for the framework.
//!
//! The core owns application components, the declarative UI tree, events,
//! layout, styling, structured concurrency, and tree diffing. Platform
//! crates (e.g. `framework-windows`) translate the resulting model into
//! native objects; **this crate never talks to an operating system** — it
//! has no `cfg(windows)`/`cfg(target_os = ...)` code and no dependency on a
//! native windowing/graphics API. Its only non-`std` dependencies are
//! portable: `tokio` (an OS-abstracted async runtime, not a UI binding),
//! `parking_lot`, and `async-trait`.
//!
//! # Getting started
//!
//! A component owns state and describes UI as a function of it; an
//! `Application` owns one or more windows' component trees; a platform
//! backend drives the whole thing. Only the last step is platform-specific.
//!
//! ```
//! use framework_core::{Application, Component, Event, Node, NodeId, Size, Window};
//!
//! struct Greeter {
//!     greeted: bool,
//! }
//!
//! impl Component for Greeter {
//!     type Props = ();
//!     type Message = ();
//!
//!     fn new((): Self::Props) -> Self {
//!         Self { greeted: false }
//!     }
//!     fn props(&self) -> &Self::Props {
//!         &()
//!     }
//!     fn set_props(&mut self, (): Self::Props) {}
//!
//!     fn view(&self) -> Node {
//!         Node::column(
//!             "root",
//!             [
//!                 Node::label("greeting", if self.greeted { "Hello!" } else { "..." }),
//!                 Node::button("greet", "Greet"),
//!             ],
//!         )
//!     }
//!
//!     fn update(&mut self, event: Event) {
//!         if matches!(event, Event::Click { target } if target == NodeId::from_key("greet")) {
//!             self.greeted = true;
//!         }
//!     }
//! }
//!
//! let mut application =
//!     Application::new(Greeter::new(()), Window::new("Greeter", Size::new(320, 200)));
//!
//! // A backend would deliver this from a real click; here it stands in for
//! // one, which is also how a headless test drives a component.
//! application.dispatch(Event::Click { target: NodeId::from_key("greet") });
//!
//! let Node::Column(root) = application.view() else { panic!("the root is a column") };
//! let Node::Label(label) = &root.children()[0] else { panic!("first child is the label") };
//! assert_eq!(label.text(), "Hello!");
//! ```
//!
//! On Windows, the last line of `main` hands it to the backend:
//!
//! ```no_run
//! # use framework_core::{Application, Component, Event, Node, Platform, Size, Window};
//! # struct Greeter;
//! # impl Component for Greeter {
//! #     type Props = ();
//! #     type Message = ();
//! #     fn new((): Self::Props) -> Self { Self }
//! #     fn props(&self) -> &Self::Props { &() }
//! #     fn set_props(&mut self, (): Self::Props) {}
//! #     fn view(&self) -> Node { Node::label("greeting", "Hello") }
//! #     fn update(&mut self, _event: Event) {}
//! # }
//! # fn run<P: Platform>(platform: &mut P) -> Result<(), P::Error> {
//! let mut application =
//!     Application::new(Greeter::new(()), Window::new("Greeter", Size::new(320, 200)));
//! // e.g. `framework_windows::WindowsPlatform::new().run(&mut application)?;`
//! platform.run(&mut application)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Module map
//!
//! | Module | Owns |
//! |---|---|
//! | [`identity`] | [`NodeId`]/[`ComponentId`]/[`WindowId`] allocation and the collision-free key interner |
//! | [`event`] | [`Event`], keyboard/accessibility types |
//! | [`node`] | The declarative [`Node`] tree a component's `view`/`render` returns |
//! | [`component`] | [`Component`], [`ComponentContext`], and the framework-managed [`ComponentTree`] |
//! | [`reconcile`] | Snapshotting a [`Node`] tree and diffing two snapshots |
//! | [`layout`] | Geometry, per-node constraints, intrinsic measurement, and the layout engine |
//! | [`style`] | [`Theme`] and per-node [`VisualStyle`] resolution |
//! | [`scheduler`] | Structured concurrency: [`Scheduler`], [`TaskScope`], the pluggable [`Executor`] backend |
//! | [`services`] | Platform-independent service contracts (HTTP, storage, clipboard, ...) |
//! | [`window`]/[`menu`] | Window-domain state and native menu definitions |
//! | [`mod@panic`] | What an application does when a component panics |
//! | [`application`] | Multi-window orchestration |
//! | [`capability`]/[`platform`] | The seam a platform backend implements and declares support through |
//!
//! Each module owns one coherent responsibility and depends only on the
//! modules "below" it in the table above; nothing here depends back up
//! toward [`application`] except [`platform`] (which must, since it is the
//! seam a backend drives an `Application` through) — see the standards
//! audit's P1.21/P2.22 findings on module boundaries, which this structure
//! is a direct response to.
//!
//! The public API re-exported below is intentionally flat
//! (`framework_core::NodeId`, `framework_core::Component`, ...) even though
//! the implementation is now modular: this keeps every existing call site
//! in `framework-windows`, the example, and application code unchanged
//! across the internal reorganization.
//!
//! # Documentation coverage
//!
//! Every public item — including individual struct fields and enum
//! variants, not just the types that contain them — carries a doc comment,
//! enforced permanently by `#![deny(missing_docs)]` below (standards audit
//! P2.23, completed in full: an earlier pass documented every type but left
//! the lint itself un-enabled, deferring roughly 380 field/variant-level
//! warnings as a "legitimate, bounded follow-up"; that follow-up is done,
//! and the lint is now `deny` rather than `warn` so it cannot silently
//! regress).
#![deny(missing_docs)]
pub mod application;
pub mod capability;
pub mod component;
pub mod event;
pub mod identity;
pub mod layout;
pub mod menu;
pub mod node;
pub mod panic;
pub mod platform;
pub mod reconcile;
pub mod scheduler;
pub mod services;
pub mod style;
pub mod window;

pub use application::Application;
pub use capability::{Capability, PlatformCapabilities};
pub use component::{
    Callback, Component, ComponentContext, ComponentHost, ComponentTree, EffectCleanup,
    EffectContext, RenderError, WindowRequests,
};
pub use event::{AccessibilityInfo, AccessibilityRole, Event, KeyCode, KeyModifiers};
pub use identity::{ComponentId, NodeId, WindowId};
pub use layout::{
    Alignment, ColumnStyle, Constraints, DefaultIntrinsicMeasurer, EdgeInsets, IntrinsicMeasurer,
    LayoutEngine, LayoutInvalidation, LayoutResult, LayoutStyle, Overflow, Point, Rect, RowStyle,
    Size, SizeMode,
};
pub use menu::{MenuBar, MenuItem};
pub use node::{Button, Column, Label, Node, NodeKind, Row, TextInput, TreeError};
pub use panic::{PanicAction, PanicPolicy, PanicReport};
pub use platform::{Platform, UnsupportedPlatform};
pub use reconcile::{TreeDiff, TreeNode, TreeOp, TreeSnapshot};
pub use scheduler::{
    Executor, ExecutorHandle, ManualExecutor, Scheduler, SleepFuture, TaskHandle, TaskId,
    TaskScope, TokioExecutor,
};
pub use services::{
    ClipboardService, FileDialogKind, FileDialogRequest, FileDialogService, HttpRequest,
    HttpResponse, HttpService, MemoryClipboard, MemoryStorage, Method, ServiceError, Services,
    StorageService, SystemService,
};
pub use style::{
    Color, ComponentStyle, ControlState, ResolvedStyle, StyleOverride, Theme, Typography,
    VisualStyle,
};
pub use window::{Window, WindowPresentation, WindowState};
