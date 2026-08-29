//! Platform-independent primitives for the framework.
//!
//! The core owns application components, the declarative UI tree, events, and
//! tree diffing. Platform crates translate the model into native objects; the
//! core never talks to an operating system.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;

use parking_lot::Mutex;

/// Stable identity for a UI node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[non_exhaustive]
pub enum Event {
    Click {
        target: NodeId,
    },
    FocusGained {
        target: NodeId,
    },
    FocusLost {
        target: NodeId,
    },
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
    WindowResized {
        window: WindowId,
        size: Size,
    },
    WindowMoved {
        window: WindowId,
        position: Point,
    },
    WindowCloseRequested {
        window: WindowId,
    },
    WindowStateChanged {
        window: WindowId,
        state: WindowPresentation,
    },
    /// A native menu item was selected. Menus are window chrome rather than
    /// part of the declarative node tree, so — like window-lifecycle events —
    /// this always routes to the window's root component rather than to a
    /// specific node owner.
    MenuAction {
        window: WindowId,
        item: NodeId,
    },
}

impl Event {
    pub const fn target(&self) -> Option<NodeId> {
        match self {
            Self::Click { target } | Self::FocusGained { target } | Self::FocusLost { target } => {
                Some(*target)
            }
            Self::KeyDown { target, .. } | Self::TextInput { target, .. } => *target,
            Self::TextChanged { target, .. } => Some(*target),
            Self::WindowResized { .. }
            | Self::WindowMoved { .. }
            | Self::WindowCloseRequested { .. }
            | Self::WindowStateChanged { .. }
            | Self::MenuAction { .. } => None,
        }
    }

    /// Rewrites a platform-facing target to the component-local key exposed
    /// to `Component::update`. Window and menu events have no node target.
    fn with_local_target(mut self, target: Option<NodeId>) -> Self {
        match &mut self {
            Self::Click { target: current }
            | Self::FocusGained { target: current }
            | Self::FocusLost { target: current }
            | Self::TextChanged {
                target: current, ..
            } => {
                if let Some(target) = target {
                    *current = target;
                }
            }
            Self::KeyDown {
                target: current, ..
            }
            | Self::TextInput {
                target: current, ..
            } => {
                *current = target;
            }
            Self::WindowResized { .. }
            | Self::WindowMoved { .. }
            | Self::WindowCloseRequested { .. }
            | Self::WindowStateChanged { .. }
            | Self::MenuAction { .. } => {}
        }
        self
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
#[non_exhaustive]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// Work performed when an effect is replaced or its component is unmounted.
///
/// Effect cleanups run before their effect-owned tasks are cancelled, allowing
/// subscriptions to be detached at a well-defined lifecycle boundary.
pub type EffectCleanup = Box<dyn FnOnce()>;

/// Capability passed to an effect after its component's render is committed.
/// Tasks spawned through this context belong to this particular effect run,
/// rather than merely to the whole component.
#[derive(Clone)]
pub struct EffectContext {
    task_scope: TaskScope,
}

/// The result type used by asynchronous platform-service contracts.
pub type ServiceFuture<T> = Pin<Box<dyn Future<Output = Result<T, ServiceError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    message: String,
}

impl ServiceError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ServiceError {}

/// A validated HTTP method. `Other` covers verbs this enum doesn't name yet
/// (e.g. WebDAV extensions) without falling back to an unvalidated `String`
/// for the common cases.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Other(String),
}

impl Method {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Other(value) => value,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Executes a single HTTP request.
///
/// `#[async_trait::async_trait]` lets this stay a plain `async fn` in the
/// trait and in every impl below; the macro desugars each into a
/// `fn(...) -> Pin<Box<dyn Future<...> + Send>>` so the trait remains
/// `dyn`-compatible for `Arc<dyn HttpService>` (see `Services` below) — no
/// more hand-written `Box::pin(async move { ... })` at every call site.
///
/// (`trait_variant::make` was tried first here and reverted: it desugars
/// `async fn` into `-> impl Future<...> + Send` instead of a boxed future,
/// which is *not* `dyn`-compatible — exactly wrong for a trait this crate
/// needs to store as `Arc<dyn _>`. Caught by `cargo check`, not by
/// inspection; see BUILD_STATUS.md.)
#[async_trait::async_trait]
pub trait HttpService: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError>;
}

#[async_trait::async_trait]
pub trait StorageService: Send + Sync {
    async fn get(&self, key: String) -> Result<Option<Vec<u8>>, ServiceError>;
    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), ServiceError>;
    async fn remove(&self, key: String) -> Result<(), ServiceError>;
}

#[async_trait::async_trait]
pub trait ClipboardService: Send + Sync {
    async fn read_text(&self) -> Result<Option<String>, ServiceError>;
    async fn write_text(&self, value: String) -> Result<(), ServiceError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDialogKind {
    OpenFile,
    SaveFile,
    PickFolder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDialogRequest {
    pub kind: FileDialogKind,
    pub title: Option<String>,
    pub filters: Vec<(String, Vec<String>)>,
}

#[async_trait::async_trait]
pub trait FileDialogService: Send + Sync {
    async fn show(&self, request: FileDialogRequest) -> Result<Option<String>, ServiceError>;
}

#[async_trait::async_trait]
pub trait SystemService: Send + Sync {
    async fn open_url(&self, url: String) -> Result<(), ServiceError>;
    async fn notify(&self, title: String, body: String) -> Result<(), ServiceError>;
}

/// Application-owned platform services. Components only receive this portable
/// contract and therefore never need to import a platform backend's API.
#[derive(Clone, Default)]
pub struct Services {
    http: Option<Arc<dyn HttpService>>,
    storage: Option<Arc<dyn StorageService>>,
    clipboard: Option<Arc<dyn ClipboardService>>,
    file_dialogs: Option<Arc<dyn FileDialogService>>,
    system: Option<Arc<dyn SystemService>>,
}

impl fmt::Debug for Services {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Services")
            .field("http", &self.http.is_some())
            .field("storage", &self.storage.is_some())
            .field("clipboard", &self.clipboard.is_some())
            .field("file_dialogs", &self.file_dialogs.is_some())
            .field("system", &self.system.is_some())
            .finish()
    }
}

impl Services {
    pub fn with_http(mut self, service: Arc<dyn HttpService>) -> Self {
        self.http = Some(service);
        self
    }
    pub fn with_storage(mut self, service: Arc<dyn StorageService>) -> Self {
        self.storage = Some(service);
        self
    }
    pub fn with_clipboard(mut self, service: Arc<dyn ClipboardService>) -> Self {
        self.clipboard = Some(service);
        self
    }
    pub fn with_file_dialogs(mut self, service: Arc<dyn FileDialogService>) -> Self {
        self.file_dialogs = Some(service);
        self
    }
    pub fn with_system(mut self, service: Arc<dyn SystemService>) -> Self {
        self.system = Some(service);
        self
    }
    pub fn http(&self) -> Option<&Arc<dyn HttpService>> {
        self.http.as_ref()
    }
    pub fn storage(&self) -> Option<&Arc<dyn StorageService>> {
        self.storage.as_ref()
    }
    pub fn clipboard(&self) -> Option<&Arc<dyn ClipboardService>> {
        self.clipboard.as_ref()
    }
    pub fn file_dialogs(&self) -> Option<&Arc<dyn FileDialogService>> {
        self.file_dialogs.as_ref()
    }
    pub fn system(&self) -> Option<&Arc<dyn SystemService>> {
        self.system.as_ref()
    }
}

/// In-memory service implementations for deterministic tests and previews.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    values: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

#[async_trait::async_trait]
impl StorageService for MemoryStorage {
    async fn get(&self, key: String) -> Result<Option<Vec<u8>>, ServiceError> {
        Ok(self.values.lock().get(&key).cloned())
    }
    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), ServiceError> {
        self.values.lock().insert(key, value);
        Ok(())
    }
    async fn remove(&self, key: String) -> Result<(), ServiceError> {
        self.values.lock().remove(&key);
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MemoryClipboard {
    value: Arc<Mutex<Option<String>>>,
}

#[async_trait::async_trait]
impl ClipboardService for MemoryClipboard {
    async fn read_text(&self) -> Result<Option<String>, ServiceError> {
        Ok(self.value.lock().clone())
    }
    async fn write_text(&self, text: String) -> Result<(), ServiceError> {
        *self.value.lock() = Some(text);
        Ok(())
    }
}

/// A portable capability exposed by a platform adapter at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Capability {
    Clipboard,
    Notifications,
    Camera,
    Bluetooth,
    Storage,
    Location,
    FileDialogs,
    SystemShare,
    UrlLaunch,
    MultipleWindows,
    WindowManagement,
    SystemAppearance,
    DragAndDrop,
    Menus,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformCapabilities {
    available: BTreeSet<Capability>,
}

impl PlatformCapabilities {
    pub fn new(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            available: capabilities.into_iter().collect(),
        }
    }
    pub fn supports(&self, capability: Capability) -> bool {
        self.available.contains(&capability)
    }
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.available.iter().copied()
    }
}

/// RGBA color token used by a theme or explicit node style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: 255,
        }
    }
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

/// Builds an opaque color from a packed `0xRRGGBB` hex value, e.g.
/// `Color::from(0xFF00FF)` for magenta. Theme tokens defined this way stay
/// `const`-constructible, matching `Color::rgb`/`Color::rgba` above.
impl From<u32> for Color {
    fn from(packed: u32) -> Self {
        Self::rgb(
            ((packed >> 16) & 0xFF) as u8,
            ((packed >> 8) & 0xFF) as u8,
            (packed & 0xFF) as u8,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typography {
    pub family: String,
    pub size: u16,
    pub weight: u16,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            family: "system-ui".into(),
            size: 14,
            weight: 400,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualStyle {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub border_radius: Option<u16>,
    pub typography: Option<Typography>,
    pub padding: Option<EdgeInsets>,
}

impl VisualStyle {
    pub const fn new() -> Self {
        Self {
            foreground: None,
            background: None,
            border: None,
            border_radius: None,
            typography: None,
            padding: None,
        }
    }
    pub const fn foreground(mut self, color: Color) -> Self {
        self.foreground = Some(color);
        self
    }
    pub const fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }
    pub const fn border(mut self, color: Color) -> Self {
        self.border = Some(color);
        self
    }
    pub const fn border_radius(mut self, radius: u16) -> Self {
        self.border_radius = Some(radius);
        self
    }
    pub fn typography(mut self, typography: Typography) -> Self {
        self.typography = Some(typography);
        self
    }
    pub const fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }
    fn merge(&self, override_style: &Self) -> Self {
        Self {
            foreground: override_style.foreground.or(self.foreground),
            background: override_style.background.or(self.background),
            border: override_style.border.or(self.border),
            border_radius: override_style.border_radius.or(self.border_radius),
            typography: override_style
                .typography
                .clone()
                .or_else(|| self.typography.clone()),
            padding: override_style.padding.or(self.padding),
        }
    }
}

impl Default for VisualStyle {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ControlState {
    #[default]
    Normal,
    Hovered,
    Focused,
    Pressed,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentStyle {
    pub normal: VisualStyle,
    pub hovered: Option<VisualStyle>,
    pub focused: Option<VisualStyle>,
    pub pressed: Option<VisualStyle>,
    pub disabled: Option<VisualStyle>,
}

impl ComponentStyle {
    pub fn resolve(&self, state: ControlState) -> VisualStyle {
        let state_style = match state {
            ControlState::Normal => None,
            ControlState::Hovered => self.hovered.as_ref(),
            ControlState::Focused => self.focused.as_ref(),
            ControlState::Pressed => self.pressed.as_ref(),
            ControlState::Disabled => self.disabled.as_ref(),
        };
        state_style
            .map(|style| self.normal.merge(style))
            .unwrap_or_else(|| self.normal.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub typography: Typography,
    pub foreground: Color,
    pub background: Color,
    pub primary: Color,
    pub spacing: u16,
    pub radius: u16,
    pub label: ComponentStyle,
    pub button: ComponentStyle,
    pub text_input: ComponentStyle,
    pub container: ComponentStyle,
}

impl Default for Theme {
    fn default() -> Self {
        let foreground = Color::rgb(32, 32, 32);
        let background = Color::rgb(255, 255, 255);
        let primary = Color::rgb(0, 120, 212);
        Self {
            typography: Typography::default(),
            foreground,
            background,
            primary,
            spacing: 8,
            radius: 4,
            label: ComponentStyle {
                normal: VisualStyle::new()
                    .foreground(foreground)
                    .typography(Typography::default()),
                ..Default::default()
            },
            button: ComponentStyle {
                normal: VisualStyle::new()
                    .foreground(foreground)
                    .background(background)
                    .border(primary)
                    .border_radius(4)
                    .typography(Typography::default()),
                ..Default::default()
            },
            text_input: ComponentStyle {
                normal: VisualStyle::new()
                    .foreground(foreground)
                    .background(background)
                    .border(Color::rgb(128, 128, 128))
                    .typography(Typography::default()),
                ..Default::default()
            },
            container: ComponentStyle {
                normal: VisualStyle::new().background(background),
                ..Default::default()
            },
        }
    }
}

impl Theme {
    pub fn resolve(
        &self,
        kind: NodeKind,
        state: ControlState,
        override_style: &VisualStyle,
    ) -> VisualStyle {
        let base = match kind {
            NodeKind::Label => &self.label,
            NodeKind::Button => &self.button,
            NodeKind::TextInput => &self.text_input,
            NodeKind::Column | NodeKind::Row => &self.container,
        }
        .resolve(state);
        base.merge(override_style)
    }
}

impl EffectContext {
    pub fn spawn<M, F>(&self, future: F) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + Send + 'static,
    {
        self.task_scope.spawn(future)
    }

    pub fn task_scope(&self) -> TaskScope {
        self.task_scope.clone()
    }

    pub fn sleep(&self, duration: Duration) -> SleepFuture {
        SleepFuture::new(duration)
    }
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

/// A deferred request to open or close a sibling window, queued by a
/// component through `ComponentContext::windows()` and applied by the
/// `Application` right after the requesting window's dispatch/task-pump
/// finishes. Opening is boxed as a constructor so `WindowRequests` stays
/// generic-free while still being able to build a strongly typed
/// `ComponentTree<C>` once it reaches the `Application` that owns the
/// window registry.
enum WindowCommand {
    Open(Box<dyn FnOnce(&mut Application) -> WindowId>),
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
    /// Requests that `window`, owned by a fresh `component`, be opened. When
    /// `modal_parent` is `Some`, the platform backend disables that window's
    /// native input while this one remains open.
    pub fn open<C: Component>(&self, component: C, window: Window, modal_parent: Option<WindowId>) {
        self.sink
            .borrow_mut()
            .push_back(WindowCommand::Open(Box::new(
                move |application: &mut Application| {
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

impl<M: Send + 'static> ComponentContext<'_, M> {
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

    /// Returns the application-owned, platform-independent service contracts.
    pub fn services(&self) -> &Services {
        &self.tree.services
    }

    /// Returns a handle for requesting that a sibling window be opened or
    /// closed. Requests are deferred and applied once this window's current
    /// dispatch or task pump finishes.
    pub fn windows(&self) -> WindowRequests {
        WindowRequests {
            sink: Rc::clone(&self.tree.window_commands),
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.tree.theme
    }

    /// Registers work that follows this component's reactive dependencies.
    ///
    /// The effect runs only after the current declarative render is committed.
    /// On a later render it is retained while `dependencies` is unchanged;
    /// when they change, its cleanup runs and its effect-owned tasks are
    /// cancelled before the replacement effect starts. Removing the component
    /// follows the same cleanup and cancellation path.
    pub fn effect<D, F>(&mut self, key: impl Into<String>, dependencies: D, effect: F)
    where
        D: Hash,
        F: FnOnce(EffectContext) -> EffectCleanup + 'static,
    {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        dependencies.hash(&mut hasher);
        self.tree
            .register_effect(self.parent, key.into(), hasher.finish(), Box::new(effect));
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
        self.tree.render_child_with_props::<C, F>(
            self.parent,
            key.into(),
            props,
            self.generation,
            constructor,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TaskId(u64);

#[derive(Debug, Clone)]
pub struct TaskHandle {
    id: TaskId,
    abort: tokio::task::AbortHandle,
    // Tracks whether *we* cancelled this task, as distinct from
    // `AbortHandle::is_finished`, which is also true after ordinary
    // completion. `TaskHandle::is_cancelled` must answer "did someone call
    // cancel()", not "is the task done".
    cancelled: Arc<AtomicBool>,
}

impl TaskHandle {
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Cancels the task. Unlike the previous cooperative-only design (an
    /// `AtomicBool` the task body had to remember to poll), this now aborts
    /// the underlying tokio task pre-emptively at its next await point, so a
    /// future that never yields cannot outlive `cancel()` on a leaked thread.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.abort.abort();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
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

/// Returns the single, process-wide tokio runtime that backs every
/// `Scheduler`/`SleepFuture` in `framework-core`.
///
/// `framework-core` still imports no platform UI API: `tokio` is a portable,
/// OS-abstracted async runtime, not a Windows/macOS/GTK binding, so this does
/// not weaken the "core must not import OS APIs" invariant (see P0.2 in the
/// standards audit). A single shared multi-threaded runtime replaces the
/// previous one-OS-thread-per-task model; tasks are cooperatively scheduled
/// on a small worker pool instead of each getting a dedicated ~MB stack.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("framework-async")
            .enable_time()
            .build()
            .expect("failed to start framework async runtime")
    })
}

#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<SchedulerInner>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SchedulerInner {
                next_id: AtomicU64::new(1),
                completed: Mutex::new(VecDeque::new()),
                waker: Mutex::new(None),
            }),
        }
    }

    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        *self.inner.waker.lock() = Some(waker);
    }

    /// Spawns `future` on the shared tokio runtime. The task runs to
    /// completion (or cancellation) off the caller's thread; its result is
    /// queued on `target` and observed the next time `Scheduler::drain` is
    /// called, exactly as before — only the execution engine changed.
    pub fn spawn<M, F>(&self, target: ComponentId, future: F) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + Send + 'static,
    {
        let id = TaskId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let cancelled = Arc::new(AtomicBool::new(false));
        let inner = Arc::clone(&self.inner);
        let join_handle = runtime().spawn(async move {
            let value = future.await;
            inner.completed.lock().push_back(CompletedTask {
                target,
                message: Box::new(value),
            });
            if let Some(waker) = inner.waker.lock().clone() {
                waker();
            }
        });
        let abort = join_handle.abort_handle();
        TaskHandle {
            id,
            abort,
            cancelled,
        }
    }

    fn drain(&self) -> Vec<CompletedTask> {
        self.inner.completed.lock().drain(..).collect()
    }
}

/// A cancellable delay. Backed by `tokio::time::sleep`; see `Scheduler` above
/// for why depending on tokio does not reintroduce a platform dependency.
pub struct SleepFuture {
    inner: Pin<Box<tokio::time::Sleep>>,
}

impl SleepFuture {
    pub fn new(duration: Duration) -> Self {
        // `tokio::time::sleep` reads the ambient runtime from a thread-local
        // at construction time, so it must be built while a runtime context
        // is entered. `Runtime::enter` just sets that thread-local for the
        // duration of the closure; it does not block or run anything.
        let _guard = runtime().enter();
        Self {
            inner: Box::pin(tokio::time::sleep(duration)),
        }
    }
}

impl Future for SleepFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}

/// Drives `future` to completion on the shared runtime from a synchronous
/// context. Private, and only used by this crate's own tests below to call
/// into the (now plain `async fn`, via `#[async_trait::async_trait]`)
/// in-memory service impls from non-`async` `#[test]` functions. This has
/// zero hand-rolled `unsafe` — `tokio::runtime::Runtime` owns a sound,
/// audited waker implementation internally.
#[cfg(test)]
fn block_on<F: Future>(future: F) -> F::Output {
    runtime().block_on(future)
}

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
    // Global native-tree ID -> component-local ID. This keeps component event
    // handlers independent of the composition path that realizes their view.
    node_ids: HashMap<NodeId, NodeId>,
    used_generation: u64,
    task_scope: TaskScope,
    effects: HashMap<String, EffectEntry>,
}

struct EffectEntry {
    dependencies: u64,
    task_scope: TaskScope,
    cleanup: Option<EffectCleanup>,
}

struct DeclaredEffect {
    key: String,
    dependencies: u64,
    run: Box<dyn FnOnce(EffectContext) -> EffectCleanup>,
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
    node_owners: HashMap<NodeId, (ComponentId, NodeId)>,
    generation: u64,
    root_view: Option<Node>,
    message_sink: Rc<RefCell<VecDeque<QueuedMessage>>>,
    window_commands: Rc<RefCell<VecDeque<WindowCommand>>>,
    scheduler: Scheduler,
    pending_effects: HashMap<ComponentId, Vec<DeclaredEffect>>,
    services: Services,
    theme: Theme,
}

impl ComponentTree {
    pub fn new<C: Component>(root: C) -> Self {
        Self::with_services(root, Services::default())
    }

    pub fn with_services<C: Component>(root: C, services: Services) -> Self {
        Self::with_services_and_theme(root, services, Theme::default())
    }

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
            generation: 0,
            root_view: None,
            message_sink: Rc::new(RefCell::new(VecDeque::new())),
            window_commands: Rc::new(RefCell::new(VecDeque::new())),
            scheduler: Scheduler::new(),
            pending_effects: HashMap::new(),
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
        tree.render();
        tree
    }

    pub fn render(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let root = self.render_component(ComponentId::ROOT, generation);
        self.root_view = Some(root);
        self.rebuild_node_owners();
        self.commit_effects(generation);
    }

    /// Returns the current rendered tree.
    ///
    /// # Panics
    ///
    /// Panics if called before the first render. In practice this cannot
    /// happen through the public API: every constructor (`new`,
    /// `with_services`, `with_services_and_theme`) performs an initial
    /// render before returning.
    pub fn view(&self) -> Node {
        self.root_view
            .clone()
            .expect("component tree must be rendered before its view is read")
    }

    pub fn dispatch(&mut self, event: Event) -> bool {
        let owner = event
            .target()
            .and_then(|target| self.node_owners.get(&target).copied());
        let (owner, event) = match owner {
            Some((owner, local)) => (owner, event.with_local_target(Some(local))),
            None => (ComponentId::ROOT, event),
        };

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
        if completed.is_empty() {
            return false;
        }
        let mut changed = false;
        for completed_task in completed {
            let Some(mut entry) = self.components.remove(&completed_task.target) else {
                continue;
            };
            if entry.component.message_any(completed_task.message) {
                entry.component.updated();
                changed = true;
            } else {
                // See `drain_messages` below for why this is not a normal,
                // ignorable outcome.
                debug_assert!(
                    false,
                    "task result for {:?} did not match its own component's Message type; \
                     this indicates a framework bug in component identity/keying, not a \
                     legitimate runtime condition",
                    completed_task.target
                );
                eprintln!(
                    "framework-core: dropped a task result for {:?} because it did not match \
                     the target component's Message type (framework bug, not application code)",
                    completed_task.target
                );
            }
            self.components.insert(completed_task.target, entry);
        }
        if changed {
            self.render();
        }
        changed
    }

    /// Drains any window-open/close requests queued through
    /// `ComponentContext::windows()` during the most recent dispatch, task
    /// pump, or render. Called by `Application`, which owns the window
    /// registry these requests act on.
    fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        self.window_commands.borrow_mut().drain(..).collect()
    }

    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    pub fn services(&self) -> &Services {
        &self.services
    }
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    fn register_effect(
        &mut self,
        component: ComponentId,
        key: String,
        dependencies: u64,
        run: Box<dyn FnOnce(EffectContext) -> EffectCleanup>,
    ) {
        let effects = self.pending_effects.entry(component).or_default();
        assert!(
            !effects.iter().any(|effect| effect.key == key),
            "a component cannot register the same effect key more than once per render: {key}"
        );
        effects.push(DeclaredEffect {
            key,
            dependencies,
            run,
        });
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
                    node_ids: HashMap::new(),
                    used_generation: generation,
                    task_scope: TaskScope::new(self.scheduler.clone(), id),
                    effects: HashMap::new(),
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
        scope_component_node_ids(&mut node, &child_node_ids, &mut node_ids);
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
                .map(|current| current.dependencies != declaration.dependencies)
                .unwrap_or(true);
            if !should_restart {
                continue;
            }
            if let Some(mut previous) = entry.effects.remove(&key) {
                Self::dispose_effect(&mut previous);
            }
            let scope = TaskScope::new(self.scheduler.clone(), id);
            let cleanup = (declaration.run)(EffectContext {
                task_scope: scope.clone(),
            });
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
        for (component, entry) in &self.components {
            for (global, local) in &entry.node_ids {
                assert!(
                    self.node_owners
                        .insert(*global, (*component, *local))
                        .is_none(),
                    "duplicate node identity after component composition: {global:?}"
                );
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

    pub fn is_mounted(&self) -> bool {
        true
    }
    pub fn view(&self) -> Node {
        self.component.view()
    }

    pub fn update(&mut self, event: Event) -> bool {
        if !self.owns_event(&event) {
            return false;
        }
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

    pub fn component(&self) -> &C {
        &self.component
    }
    pub fn component_mut(&mut self) -> &mut C {
        &mut self.component
    }

    pub fn replace(&mut self, mut component: C) {
        self.component.unmounted();
        component.mounted();
        self.component = component;
    }
}

impl<C: Component> Drop for ComponentHost<C> {
    fn drop(&mut self) {
        self.component.unmounted();
    }
}

pub struct Application {
    windows: HashMap<WindowId, WindowEntry>,
    primary_window: WindowId,
    next_window_id: u64,
    services: Services,
    theme: Theme,
}

struct WindowEntry {
    window: Window,
    state: WindowState,
    components: ComponentTree,
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
        let mut application = Self {
            windows,
            primary_window,
            next_window_id: 1,
            services,
            theme,
        };
        // A component may request another window from its first render.  The
        // root tree is rendered while this Application is being constructed,
        // so drain those requests only after the primary entry is installed.
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
    pub fn view(&self) -> Node {
        self.view_for(self.primary_window)
            .expect("primary window must exist")
    }
    pub fn view_for(&self, id: WindowId) -> Option<Node> {
        self.windows.get(&id).map(|entry| entry.components.view())
    }
    pub fn render(&mut self) {
        self.render_window(self.primary_window);
    }
    pub fn render_window(&mut self, id: WindowId) {
        if let Some(entry) = self.windows.get_mut(&id) {
            entry.components.render();
        }
        // Explicit renders have the same deferred-command guarantee as an
        // event or task transaction.  Without this, a request made during a
        // first/manual render would wait for an unrelated later event.
        self.apply_queued_window_commands();
    }
    pub fn components(&self) -> &ComponentTree {
        &self.windows[&self.primary_window].components
    }
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
    pub fn scheduler(&self) -> &Scheduler {
        self.components().scheduler()
    }
    pub fn scheduler_for(&self, id: WindowId) -> Option<&Scheduler> {
        self.windows
            .get(&id)
            .map(|entry| entry.components.scheduler())
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
                state: WindowState {
                    modal_parent,
                    ..WindowState::new(window.size())
                },
                window,
                components: ComponentTree::with_services_and_theme(
                    component,
                    self.services.clone(),
                    self.theme.clone(),
                ),
            },
        );
        // The new root has already rendered and may itself have queued
        // follow-up requests.  Apply them now so initial rendering is a
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

        // A modal child cannot remain alive after its parent disappears: its
        // native backend would otherwise retain a dangling modal relationship
        // and the application would report an impossible window topology.
        let mut pending = vec![id];
        let mut closing = HashSet::new();
        while let Some(current) = pending.pop() {
            if !closing.insert(current) {
                continue;
            }
            pending.extend(self.windows.iter().filter_map(|(child, entry)| {
                (entry.state.modal_parent == Some(current)).then_some(*child)
            }));
        }
        for window in closing {
            self.windows.remove(&window);
        }
        true
    }

    pub fn window_ids(&self) -> Vec<WindowId> {
        let mut ids = self.windows.keys().copied().collect::<Vec<_>>();
        ids.sort();
        ids
    }

    pub fn window_state(&self, id: WindowId) -> Option<&WindowState> {
        self.windows.get(&id).map(|entry| &entry.state)
    }
    pub fn window_state_mut(&mut self, id: WindowId) -> Option<&mut WindowState> {
        self.windows.get_mut(&id).map(|entry| &mut entry.state)
    }
    pub fn services(&self) -> &Services {
        &self.services
    }
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn window(&self) -> &Window {
        &self.windows[&self.primary_window].window
    }
    pub fn window_for(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id).map(|entry| &entry.window)
    }

    fn allocate_window_id(&mut self) -> WindowId {
        let start = self.next_window_id;
        loop {
            let candidate = WindowId(self.next_window_id);
            self.next_window_id = self.next_window_id.wrapping_add(1).max(1);
            if !self.windows.contains_key(&candidate) {
                return candidate;
            }
            assert!(
                self.next_window_id != start,
                "all non-primary WindowId values are exhausted"
            );
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WindowId(u64);

impl WindowId {
    pub const PRIMARY: Self = Self(0);
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowPresentation {
    #[default]
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowState {
    pub size: Size,
    pub position: Point,
    pub presentation: WindowPresentation,
    pub visible: bool,
    pub modal_parent: Option<WindowId>,
}

impl WindowState {
    pub const fn new(size: Size) -> Self {
        Self {
            size,
            position: Point::new(0, 0),
            presentation: WindowPresentation::Normal,
            visible: true,
            modal_parent: None,
        }
    }
}

/// Returns the window named by a window-lifecycle event. UI input events are
/// routed by their node ownership and therefore have no window ID here.
fn event_window_id(event: &Event) -> Option<WindowId> {
    match event {
        Event::WindowResized { window, .. }
        | Event::WindowMoved { window, .. }
        | Event::WindowCloseRequested { window }
        | Event::WindowStateChanged { window, .. }
        | Event::MenuAction { window, .. } => Some(*window),
        _ => None,
    }
}

/// Keeps the application-owned lifecycle snapshot authoritative before the
/// component observes the corresponding framework event.
fn apply_window_event(state: &mut WindowState, event: &Event) {
    match event {
        Event::WindowResized { size, .. } => state.size = *size,
        Event::WindowMoved { position, .. } => state.position = *position,
        Event::WindowStateChanged {
            state: presentation,
            ..
        } => {
            state.presentation = *presentation;
            state.visible = *presentation != WindowPresentation::Minimized;
        }
        Event::WindowCloseRequested { .. } => {}
        _ => {}
    }
}

#[derive(Debug, Clone)]
pub struct Window {
    title: String,
    size: Size,
    menu: Option<MenuBar>,
}

impl Window {
    pub fn new(title: impl Into<String>, size: Size) -> Self {
        Self {
            title: title.into(),
            size,
            menu: None,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Attaches a native menu bar realized by the platform backend when this
    /// window is created. The menu is part of the window's static
    /// definition, the same maturity level as its title and initial size:
    /// changing it after the window opens is not yet supported.
    pub fn with_menu(mut self, menu: MenuBar) -> Self {
        self.menu = Some(menu);
        self
    }

    pub fn menu(&self) -> Option<&MenuBar> {
        self.menu.as_ref()
    }
}

/// A single entry in a native `MenuBar`. Leaf items (those with no children)
/// dispatch `Event::MenuAction { item, .. }` to the window's root component
/// when selected, using the same stable `NodeId::from_key` identity already
/// used to target UI nodes. Items with children render as a native submenu
/// instead of being individually actionable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    id: NodeId,
    label: String,
    enabled: bool,
    checked: Option<bool>,
    separator: bool,
    children: Vec<MenuItem>,
}

impl MenuItem {
    /// A leaf menu item that dispatches `Event::MenuAction` with the id
    /// derived from `key` when selected.
    pub fn action(key: impl AsRef<str>, label: impl Into<String>) -> Self {
        Self {
            id: NodeId::from_key(key.as_ref()),
            label: label.into(),
            enabled: true,
            checked: None,
            separator: false,
            children: Vec::new(),
        }
    }

    /// A submenu item that groups other items instead of dispatching an
    /// action itself.
    pub fn submenu(
        key: impl AsRef<str>,
        label: impl Into<String>,
        children: impl IntoIterator<Item = MenuItem>,
    ) -> Self {
        Self {
            id: NodeId::from_key(key.as_ref()),
            label: label.into(),
            enabled: true,
            checked: None,
            separator: false,
            children: children.into_iter().collect(),
        }
    }

    /// A non-actionable visual divider between items in the same menu.
    pub fn separator() -> Self {
        Self {
            id: NodeId::from_key(""),
            label: String::new(),
            enabled: true,
            checked: None,
            separator: true,
            children: Vec::new(),
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Renders the item with a checkmark. Passing `false` renders an
    /// explicit (unchecked) checkable item rather than an ordinary one; use
    /// `action`/`submenu` alone to opt out of the checkable presentation.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub fn is_checked(&self) -> Option<bool> {
        self.checked
    }
    pub fn is_separator(&self) -> bool {
        self.separator
    }
    pub fn is_submenu(&self) -> bool {
        !self.children.is_empty()
    }
    pub fn children(&self) -> &[MenuItem] {
        &self.children
    }
}

/// A portable, declarative native menu bar attached to a `Window`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuBar {
    items: Vec<MenuItem>,
}

impl MenuBar {
    pub fn new(items: impl IntoIterator<Item = MenuItem>) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }

    pub fn items(&self) -> &[MenuItem] {
        &self.items
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
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeMode {
    #[default]
    Auto,
    Fixed(i32),
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    Visible,
    #[default]
    Clip,
    Scroll,
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
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub const fn symmetric(vertical: i32, horizontal: i32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
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

/// Min/max size bounds for a node's layout. Fields are private and every
/// constructor enforces `0 <= min <= max` (when a max is set) so an invalid
/// `Constraints` (e.g. `max_width < min_width`, previously constructible
/// directly since the fields were `pub`) cannot be built at all, rather than
/// being silently re-clamped at every place a value is measured against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Constraints {
    min_width: i32,
    max_width: Option<i32>,
    min_height: i32,
    max_height: Option<i32>,
}

impl Constraints {
    pub const fn new() -> Self {
        Self {
            min_width: 0,
            max_width: None,
            min_height: 0,
            max_height: None,
        }
    }

    pub const fn with_min_width(mut self, value: i32) -> Self {
        self.min_width = non_negative(value);
        self.max_width = raise_to(self.max_width, self.min_width);
        self
    }
    pub const fn with_max_width(mut self, value: i32) -> Self {
        let value = non_negative(value);
        self.max_width = Some(if value > self.min_width {
            value
        } else {
            self.min_width
        });
        self
    }
    pub const fn with_min_height(mut self, value: i32) -> Self {
        self.min_height = non_negative(value);
        self.max_height = raise_to(self.max_height, self.min_height);
        self
    }
    pub const fn with_max_height(mut self, value: i32) -> Self {
        let value = non_negative(value);
        self.max_height = Some(if value > self.min_height {
            value
        } else {
            self.min_height
        });
        self
    }

    pub const fn min_width(&self) -> i32 {
        self.min_width
    }
    pub const fn max_width(&self) -> Option<i32> {
        self.max_width
    }
    pub const fn min_height(&self) -> i32 {
        self.min_height
    }
    pub const fn max_height(&self) -> Option<i32> {
        self.max_height
    }

    pub const fn clamp_width(self, value: i32) -> i32 {
        let value = if value < self.min_width {
            self.min_width
        } else {
            value
        };
        match self.max_width {
            // SAFETY invariant established by the constructors above:
            // `max_width >= min_width` always holds here, so no defensive
            // re-clamp of `max` against `min` is needed at this use site.
            Some(max) if value > max => max,
            _ => value,
        }
    }

    pub const fn clamp_height(self, value: i32) -> i32 {
        let value = if value < self.min_height {
            self.min_height
        } else {
            value
        };
        match self.max_height {
            Some(max) if value > max => max,
            _ => value,
        }
    }
}

const fn non_negative(value: i32) -> i32 {
    if value > 0 { value } else { 0 }
}

const fn raise_to(max: Option<i32>, min: i32) -> Option<i32> {
    match max {
        Some(max) if max < min => Some(min),
        other => other,
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
            margin: EdgeInsets {
                top: 0,
                right: 0,
                bottom: 0,
                left: 0,
            },
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
            padding: EdgeInsets {
                top: 24,
                right: 24,
                bottom: 24,
                left: 24,
            },
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
            padding: EdgeInsets {
                top: 24,
                right: 24,
                bottom: 24,
                left: 24,
            },
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

/// The resolved container layout parameters `layout_column`/`layout_row`
/// need. Grouping `padding`/`gap`/`align_items` here (rather than passing
/// each positionally) removes the last argument-order footgun between two
/// same-typed `i32`/`Alignment` parameters and is what lets both functions
/// drop `#[allow(clippy::too_many_arguments)]` entirely instead of
/// suppressing the lint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedContainerStyle {
    padding: EdgeInsets,
    gap: i32,
    align_items: Alignment,
}

impl From<ColumnStyle> for ResolvedContainerStyle {
    fn from(style: ColumnStyle) -> Self {
        Self {
            padding: style.padding,
            gap: style.gap,
            align_items: style.align_items,
        }
    }
}

impl From<RowStyle> for ResolvedContainerStyle {
    fn from(style: RowStyle) -> Self {
        Self {
            padding: style.padding,
            gap: style.gap,
            align_items: style.align_items,
        }
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
    pub const fn none() -> Self {
        Self { full: false }
    }
    pub const fn full() -> Self {
        Self { full: true }
    }
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
    pub const fn new() -> Self {
        Self
    }

    pub fn layout(&self, snapshot: &TreeSnapshot, size: Size) -> HashMap<NodeId, Rect> {
        self.layout_with(snapshot, size, &DefaultIntrinsicMeasurer)
    }

    pub fn layout_with<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        size: Size,
        measurer: &M,
    ) -> HashMap<NodeId, Rect> {
        self.layout_result_with(snapshot, size, measurer, &HashMap::new())
            .rects
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
                    result
                        .clips
                        .insert(node.id, Rect::new(0, 0, rect.width, rect.height));
                }
                let content_size = self.layout_column(
                    snapshot,
                    content_rect,
                    children,
                    style.into(),
                    result,
                    measurer,
                );
                result.content_sizes.insert(node.id, content_size);
                result.scroll_ranges.insert(
                    node.id,
                    Size::new(
                        content_size.width.saturating_sub(rect.width.max(0) as u32),
                        content_size
                            .height
                            .saturating_sub(rect.height.max(0) as u32),
                    ),
                );
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                if !matches!(style.overflow, Overflow::Visible) {
                    result
                        .clips
                        .insert(node.id, Rect::new(0, 0, rect.width, rect.height));
                }
                let content_size = self.layout_row(
                    snapshot,
                    content_rect,
                    children,
                    style.into(),
                    result,
                    measurer,
                );
                result.content_sizes.insert(node.id, content_size);
                result.scroll_ranges.insert(
                    node.id,
                    Size::new(
                        content_size.width.saturating_sub(rect.width.max(0) as u32),
                        content_size
                            .height
                            .saturating_sub(rect.height.max(0) as u32),
                    ),
                );
            }
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {}
        }
    }

    fn layout_column<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        rect: Rect,
        children: Vec<&TreeNode>,
        style: ResolvedContainerStyle,
        result: &mut LayoutResult,
        measurer: &M,
    ) -> Size {
        let ResolvedContainerStyle {
            padding,
            gap,
            align_items,
        } = style;
        let content = inner_rect(rect, padding);
        if children.is_empty() {
            return Size::new(
                padding.horizontal().max(0) as u32,
                padding.vertical().max(0) as u32,
            );
        }

        let gap_total = gap.max(0) * children.len().saturating_sub(1) as i32;
        let usable_height = (content.height - gap_total).max(0);
        let preferred_height = children
            .iter()
            .map(|child| {
                self.preferred_height(snapshot, child, measurer, preferred_width_hint(child))
            })
            .sum::<i32>();
        let fill_count = children
            .iter()
            .filter(|child| matches!(child.layout.height, SizeMode::Fill))
            .count();
        let distributable = (usable_height - preferred_height).max(0);
        let share = if fill_count == 0 {
            0
        } else {
            distributable / fill_count as i32
        };
        let remainder = if fill_count == 0 {
            0
        } else {
            distributable % fill_count as i32
        };

        let mut y = content.y;
        let mut fill_index = 0;
        for (index, child) in children.iter().enumerate() {
            let margin = child.layout.margin;
            y += margin.top;
            let height = match child.layout.height {
                SizeMode::Fixed(value) => value.max(0),
                SizeMode::Auto => {
                    self.preferred_height(snapshot, child, measurer, preferred_width_hint(child))
                }
                SizeMode::Fill => {
                    let extra = if fill_index == 0 { remainder } else { 0 };
                    fill_index += 1;
                    (self.preferred_height(snapshot, child, measurer, preferred_width_hint(child))
                        + share
                        + extra)
                        .max(0)
                }
            };

            let available_width = (content.width - margin.horizontal()).max(0);
            let alignment = child.layout.align_self.unwrap_or(align_items);
            let width = match (alignment, child.layout.width) {
                (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available_width,
                (_, SizeMode::Auto) => self
                    .preferred_width(snapshot, child, measurer)
                    .min(available_width),
                (_, mode) => resolve_width(mode, available_width),
            };
            let width = child
                .layout
                .constraints
                .clamp_width(width.max(0))
                .min(available_width.max(0));
            let height = child.layout.constraints.clamp_height(height.max(0));
            let x = aligned_start(content.x, margin.left, available_width, width, alignment);
            let child_rect = Rect::new(x, y, width, height);
            self.layout_node(snapshot, child, child_rect, result, measurer);

            y += height + margin.bottom;
            if index + 1 < children.len() {
                y += gap.max(0);
            }
        }

        // Content size must be computed from the unscrolled layout. Scrolling
        // changes the viewport position of children; it must never change the
        // size of the content itself or the available scroll range.
        let natural_height = (y + padding.bottom).max(rect.height);
        let natural_width = children
            .iter()
            .map(|child| {
                result
                    .rects
                    .get(&child.id)
                    .map(|r| r.x + r.width + child.layout.margin.right)
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0)
            .max(rect.width);

        Size::new(natural_width.max(0) as u32, natural_height.max(0) as u32)
    }

    fn layout_row<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        rect: Rect,
        children: Vec<&TreeNode>,
        style: ResolvedContainerStyle,
        result: &mut LayoutResult,
        measurer: &M,
    ) -> Size {
        let ResolvedContainerStyle {
            padding,
            gap,
            align_items,
        } = style;
        let content = inner_rect(rect, padding);
        if children.is_empty() {
            return Size::new(
                padding.horizontal().max(0) as u32,
                padding.vertical().max(0) as u32,
            );
        }

        let gap_total = gap.max(0) * children.len().saturating_sub(1) as i32;
        let usable_width = (content.width - gap_total).max(0);
        let preferred_width = children
            .iter()
            .map(|child| self.preferred_width(snapshot, child, measurer))
            .sum::<i32>();
        let fill_count = children
            .iter()
            .filter(|child| matches!(child.layout.width, SizeMode::Fill))
            .count();
        let distributable = (usable_width - preferred_width).max(0);
        let share = if fill_count == 0 {
            0
        } else {
            distributable / fill_count as i32
        };
        let remainder = if fill_count == 0 {
            0
        } else {
            distributable % fill_count as i32
        };

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
                (_, SizeMode::Auto) => self
                    .preferred_height(snapshot, child, measurer, Some(width))
                    .min(available_height),
                (_, mode) => resolve_width(mode, available_height),
            };
            let width = child.layout.constraints.clamp_width(width.max(0));
            let height = child
                .layout
                .constraints
                .clamp_height(height.max(0))
                .min(available_height.max(0));
            let y = aligned_start(content.y, margin.top, available_height, height, alignment);
            let child_rect = Rect::new(x, y, width, height);
            self.layout_node(snapshot, child, child_rect, result, measurer);

            x += width + margin.right;
            if index + 1 < children.len() {
                x += gap.max(0);
            }
        }

        // As with Column, measure the unscrolled content first. The scroll
        // offset is a viewport transform and must not feed back into the
        // intrinsic content size.
        let natural_width = (x + padding.right).max(rect.width);
        let natural_height = children
            .iter()
            .map(|child| {
                result
                    .rects
                    .get(&child.id)
                    .map(|r| r.y + r.height + child.layout.margin.bottom)
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0)
            .max(rect.height);

        Size::new(natural_width.max(0) as u32, natural_height.max(0) as u32)
    }

    fn preferred_width<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
    ) -> i32 {
        let base = match node.kind {
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {
                measurer
                    .measure(node.kind, node.text.as_deref(), None)
                    .width as i32
            }
            NodeKind::Column | NodeKind::Row => {
                self.container_preferred_width(snapshot, node, measurer)
            }
        };
        node.layout
            .constraints
            .clamp_width(base + node.layout.margin.horizontal())
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
                measurer
                    .measure(node.kind, node.text.as_deref(), max_width)
                    .height as i32
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
                        .map(|child| {
                            self.preferred_height(
                                snapshot,
                                child,
                                measurer,
                                preferred_width_hint(child).or(max_width),
                            )
                        })
                        .max()
                        .unwrap_or(0)
            }
        }
    }

    fn container_preferred_width<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
    ) -> i32 {
        let children = ordered_children(snapshot, node.id);
        match node.kind {
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                style.padding.horizontal()
                    + children
                        .iter()
                        .map(|child| self.preferred_width(snapshot, child, measurer))
                        .max()
                        .unwrap_or(0)
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                style.padding.horizontal()
                    + children
                        .iter()
                        .map(|child| self.preferred_width(snapshot, child, measurer))
                        .sum::<i32>()
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

fn ordered_children(snapshot: &TreeSnapshot, parent: NodeId) -> Vec<&TreeNode> {
    let mut children = snapshot
        .nodes()
        .filter(|candidate| candidate.parent == Some(parent))
        .collect::<Vec<_>>();
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

fn aligned_start(
    origin: i32,
    margin_start: i32,
    available: i32,
    size: i32,
    alignment: Alignment,
) -> i32 {
    let remaining = (available - size).max(0);
    origin
        + margin_start
        + match alignment {
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
        Self::TextInput(TextInput::new(
            NodeId::from_key(key.as_ref()),
            value,
            layout,
        ))
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
            Self::Label(mut node) => {
                node.accessibility = accessibility;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.accessibility = accessibility;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.accessibility = accessibility;
                Self::TextInput(node)
            }
            Self::Column(mut node) => {
                node.accessibility = accessibility;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.accessibility = accessibility;
                Self::Row(node)
            }
        }
    }

    /// Applies a node-level visual override. The active theme supplies any
    /// unspecified values when the backend resolves the style.
    pub fn with_style(self, style: VisualStyle) -> Self {
        match self {
            Self::Label(mut node) => {
                node.visual_style = style;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.visual_style = style;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.visual_style = style;
                Self::TextInput(node)
            }
            Self::Column(mut node) => {
                node.visual_style = style;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.visual_style = style;
                Self::Row(node)
            }
        }
    }

    pub fn visual_style(&self) -> &VisualStyle {
        match self {
            Self::Label(node) => &node.visual_style,
            Self::Button(node) => &node.visual_style,
            Self::TextInput(node) => &node.visual_style,
            Self::Column(node) => &node.visual_style,
            Self::Row(node) => &node.visual_style,
        }
    }

    /// Marks this node (and, for containers, its native realization) as
    /// disabled. A disabled control stops accepting native input focus and
    /// input events, is skipped by Tab/Shift+Tab traversal, and is styled
    /// using the theme's `ControlState::Disabled` variant.
    pub fn disabled(self, disabled: bool) -> Self {
        match self {
            Self::Label(mut node) => {
                node.disabled = disabled;
                Self::Label(node)
            }
            Self::Button(mut node) => {
                node.disabled = disabled;
                Self::Button(node)
            }
            Self::TextInput(mut node) => {
                node.disabled = disabled;
                Self::TextInput(node)
            }
            Self::Column(mut node) => {
                node.disabled = disabled;
                Self::Column(node)
            }
            Self::Row(mut node) => {
                node.disabled = disabled;
                Self::Row(node)
            }
        }
    }

    pub fn is_disabled(&self) -> bool {
        match self {
            Self::Label(node) => node.disabled,
            Self::Button(node) => node.disabled,
            Self::TextInput(node) => node.disabled,
            Self::Column(node) => node.disabled,
            Self::Row(node) => node.disabled,
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

/// Applies a component's stable identity to the nodes it owns. Nodes returned
/// by managed children have already been scoped, so their complete subtrees
/// are left intact when their parent is scoped.
fn scope_component_node_ids(
    node: &mut Node,
    child_node_ids: &HashSet<NodeId>,
    node_ids: &mut HashMap<NodeId, NodeId>,
) {
    let local = node.id();
    if child_node_ids.contains(&local) {
        return;
    }

    // A node's identity stays the developer's own key: `Component::update`,
    // `TreeSnapshot::get`, and platform-delivered events all address nodes by
    // this same `NodeId`, so it must not be rewritten per owning component.
    // Two different components using the same literal key is still caught
    // below, exactly as it would be for two nodes within a single view.
    let global = local;
    node.set_id(global);
    assert!(
        node_ids.insert(global, local).is_none(),
        "a component cannot use the same node key more than once: {local:?}"
    );

    match node {
        Node::Column(column) => {
            for child in &mut column.children {
                scope_component_node_ids(child, child_node_ids, node_ids);
            }
        }
        Node::Row(row) => {
            for child in &mut row.children {
                scope_component_node_ids(child, child_node_ids, node_ids);
            }
        }
        Node::Label(_) | Node::Button(_) | Node::TextInput(_) => {}
    }
}

impl Node {
    fn set_id(&mut self, id: NodeId) {
        match self {
            Self::Label(node) => node.id = id,
            Self::Button(node) => node.id = id,
            Self::TextInput(node) => node.id = id,
            Self::Column(node) => node.id = id,
            Self::Row(node) => node.id = id,
        }
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
    visual_style: VisualStyle,
    disabled: bool,
}

impl Label {
    fn new(id: NodeId, text: impl Into<String>, layout: LayoutStyle) -> Self {
        let text = text.into();
        let accessibility = AccessibilityInfo::new(AccessibilityRole::Label)
            .name(text.clone())
            .focusable(false);
        Self {
            id,
            text,
            layout,
            accessibility,
            visual_style: VisualStyle::default(),
            disabled: false,
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn layout(&self) -> LayoutStyle {
        self.layout
    }
    pub fn accessibility(&self) -> &AccessibilityInfo {
        &self.accessibility
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    id: NodeId,
    text: String,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
    visual_style: VisualStyle,
    disabled: bool,
}

impl Button {
    fn new(id: NodeId, text: impl Into<String>, layout: LayoutStyle) -> Self {
        let text = text.into();
        let accessibility = AccessibilityInfo::new(AccessibilityRole::Button)
            .name(text.clone())
            .focusable(true);
        Self {
            id,
            text,
            layout,
            accessibility,
            visual_style: VisualStyle::default(),
            disabled: false,
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn layout(&self) -> LayoutStyle {
        self.layout
    }
    pub fn accessibility(&self) -> &AccessibilityInfo {
        &self.accessibility
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput {
    id: NodeId,
    value: String,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
    visual_style: VisualStyle,
    disabled: bool,
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
            visual_style: VisualStyle::default(),
            disabled: false,
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn value(&self) -> &str {
        &self.value
    }
    pub fn layout(&self) -> LayoutStyle {
        self.layout
    }
    pub fn accessibility(&self) -> &AccessibilityInfo {
        &self.accessibility
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    id: NodeId,
    children: Vec<Node>,
    style: ColumnStyle,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
    visual_style: VisualStyle,
    disabled: bool,
}

impl Column {
    fn new(id: NodeId, children: Vec<Node>, style: ColumnStyle, layout: LayoutStyle) -> Self {
        Self {
            id,
            children,
            style,
            layout,
            accessibility: AccessibilityInfo::new(AccessibilityRole::Group).focusable(false),
            visual_style: VisualStyle::default(),
            disabled: false,
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn children(&self) -> &[Node] {
        &self.children
    }
    pub fn style(&self) -> ColumnStyle {
        self.style
    }
    pub fn layout(&self) -> LayoutStyle {
        self.layout
    }
    pub fn accessibility(&self) -> &AccessibilityInfo {
        &self.accessibility
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    id: NodeId,
    children: Vec<Node>,
    style: RowStyle,
    layout: LayoutStyle,
    accessibility: AccessibilityInfo,
    visual_style: VisualStyle,
    disabled: bool,
}

impl Row {
    fn new(id: NodeId, children: Vec<Node>, style: RowStyle, layout: LayoutStyle) -> Self {
        Self {
            id,
            children,
            style,
            layout,
            accessibility: AccessibilityInfo::new(AccessibilityRole::Group).focusable(false),
            visual_style: VisualStyle::default(),
            disabled: false,
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
    pub fn children(&self) -> &[Node] {
        &self.children
    }
    pub fn style(&self) -> RowStyle {
        self.style
    }
    pub fn layout(&self) -> LayoutStyle {
        self.layout
    }
    pub fn accessibility(&self) -> &AccessibilityInfo {
        &self.accessibility
    }
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
    /// The node's unresolved visual override. This remains available to a
    /// platform backend so it can resolve live native interaction states
    /// (such as focus) without losing per-node customization.
    pub style_override: VisualStyle,
    pub visual_style: VisualStyle,
    pub disabled: bool,
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
            style_override: node.visual_style().clone(),
            visual_style: node.visual_style().clone(),
            disabled: node.is_disabled(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeSnapshot {
    nodes: HashMap<NodeId, TreeNode>,
}

impl TreeSnapshot {
    /// Builds an immutable snapshot of `root` for the layout/hit-testing
    /// passes to read from.
    ///
    /// # Errors
    ///
    /// Returns `TreeError::DuplicateNodeId` if two nodes in `root` share the
    /// same `NodeId` — every node key must be unique within a single view.
    pub fn from_node(root: &Node) -> Result<Self, TreeError> {
        root.validate_unique_ids()?;

        let mut nodes = HashMap::new();
        root.visit(&mut |node, parent, index| {
            nodes.insert(node.id(), TreeNode::from_node(node, parent, index));
        });

        Ok(Self { nodes })
    }

    /// Builds a snapshot whose `visual_style` on every node is fully resolved
    /// against `theme`: the theme's per-kind default merged with that node's
    /// own override, in the node's `Normal` (or `Disabled`, when the node is
    /// marked disabled) state. A platform backend can apply the resulting
    /// colors and typography directly without needing to know about `Theme`
    /// merge order itself, mirroring how layout geometry is already fully
    /// resolved in the core before a backend applies it.
    ///
    /// Interactive states that only the backend can observe live — hover,
    /// press, and focus — remain the backend's responsibility: it already
    /// tracks focus for Tab traversal and can re-resolve a single focused
    /// node's style against the same theme when focus changes.
    ///
    /// # Errors
    ///
    /// Returns `TreeError::DuplicateNodeId` under the same condition as
    /// `from_node` above.
    pub fn from_node_with_theme(root: &Node, theme: &Theme) -> Result<Self, TreeError> {
        let mut snapshot = Self::from_node(root)?;
        for node in snapshot.nodes.values_mut() {
            let state = if node.disabled {
                ControlState::Disabled
            } else {
                ControlState::Normal
            };
            node.visual_style = theme.resolve(node.kind, state, &node.visual_style);
        }
        Ok(snapshot)
    }

    pub fn get(&self, id: NodeId) -> Option<&TreeNode> {
        self.nodes.get(&id)
    }
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }
    pub fn nodes(&self) -> impl Iterator<Item = &TreeNode> {
        self.nodes.values()
    }

    /// Returns nodes in declarative preorder, preserving sibling indices.
    pub fn ordered_nodes(&self) -> Vec<&TreeNode> {
        let mut ordered = Vec::with_capacity(self.nodes.len());
        let root = self
            .nodes
            .values()
            .find(|node| node.parent.is_none())
            .map(|node| node.id);
        if let Some(root) = root {
            collect_ordered_nodes(self, root, &mut ordered);
        }
        ordered
    }
}

fn collect_ordered_nodes<'a>(
    snapshot: &'a TreeSnapshot,
    id: NodeId,
    output: &mut Vec<&'a TreeNode>,
) {
    let Some(node) = snapshot.get(id) else {
        return;
    };
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
                || previous_node.accessibility != node.accessibility
                || previous_node.style_override != node.style_override
                || previous_node.visual_style != node.visual_style
                || previous_node.disabled != node.disabled
            {
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
        self.operations.iter().any(|operation| {
            matches!(
                operation,
                TreeOp::Insert(_) | TreeOp::Remove(_) | TreeOp::Move { .. } | TreeOp::Update(_)
            )
        })
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

    /// Runs the native event loop until the application exits.
    ///
    /// # Errors
    ///
    /// Returns `Self::Error` if the platform backend fails to initialize or
    /// encounters an unrecoverable native error while running — for
    /// `framework-windows`, this includes a component panicking inside a
    /// `WNDPROC` callback (see `Error::ComponentPanicked`).
    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error>;

    /// Reports the portable features available from this adapter at runtime.
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::default()
    }

    /// Explicit escape hatch for backend-specific functionality. Applications
    /// may downcast this value at their platform boundary without allowing
    /// platform types to leak into framework-core.
    fn native_extension(&self) -> &dyn Any;
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
        tree.dispatch(Event::Click {
            target: NodeId::from_key("managed-child"),
        });
        let snapshot = TreeSnapshot::from_node(&tree.view()).unwrap();
        assert_eq!(
            snapshot
                .get(NodeId::from_key("managed-child"))
                .and_then(|node| node.text.as_deref()),
            Some("child count: 1")
        );
        assert_eq!(mounts.get(), 1);
        assert_eq!(unmounts.get(), 0);

        tree.dispatch(Event::Click {
            target: NodeId::from_key("toggle-managed-child"),
        });
        assert_eq!(unmounts.get(), 1);

        tree.dispatch(Event::Click {
            target: NodeId::from_key("toggle-managed-child"),
        });
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
            self.props
                .prop_changes
                .set(self.props.prop_changes.get() + 1);
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
        assert_eq!(
            snapshot
                .get(NodeId::from_key("prop-title"))
                .unwrap()
                .text
                .as_deref(),
            Some("First title")
        );

        tree.dispatch(Event::Click {
            target: NodeId::from_key("prop-count"),
        });
        tree.dispatch(Event::Click {
            target: NodeId::from_key("change-props"),
        });

        let snapshot = TreeSnapshot::from_node(&tree.view()).unwrap();
        assert_eq!(
            snapshot
                .get(NodeId::from_key("prop-title"))
                .unwrap()
                .text
                .as_deref(),
            Some("Second title")
        );
        assert_eq!(
            snapshot
                .get(NodeId::from_key("prop-count"))
                .unwrap()
                .text
                .as_deref(),
            Some("count: 1")
        );

        // The keyed child keeps its instance/state while receiving new props.
        assert_eq!(mounts.get(), 1);
        assert_eq!(prop_changes.get(), 1);
        assert_eq!(
            tree.components
                .values()
                .filter(|entry| entry.component_type == std::any::TypeId::of::<PropChild>())
                .count(),
            1
        );
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
                Node::button("child-action", format!("Child {}", self.count))
            }
            fn update(&mut self, event: Event) {
                if matches!(event, Event::Click { target } if target == NodeId::from_key("child-action"))
                {
                    self.count += 1;
                    self.props
                        .callback
                        .send(ParentMessage::ChildCount(self.count));
                }
            }
            fn mounted(&mut self) {
                self.props.mounts.set(self.props.mounts.get() + 1);
            }
        }

        #[derive(Clone, PartialEq, Debug)]
        enum ParentMessage {
            ChildCount(u32),
        }

        struct Parent {
            observed: u32,
            mounts: Rc<Cell<u32>>,
        }

        impl Component for Parent {
            type Props = ();
            type Message = ParentMessage;
            fn new(_: Self::Props) -> Self {
                Self {
                    observed: 0,
                    mounts: Rc::new(Cell::new(0)),
                }
            }
            fn props(&self) -> &Self::Props {
                static PROPS: () = ();
                &PROPS
            }
            fn set_props(&mut self, _: Self::Props) {}
            fn view(&self) -> Node {
                Node::label("parent-observed", format!("Observed {}", self.observed))
            }
            fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
                let callback = context.callback();
                let child = context.child_with_props(
                    "child",
                    ChildProps {
                        callback,
                        mounts: self.mounts.clone(),
                    },
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
                match message {
                    ParentMessage::ChildCount(count) => self.observed = count,
                }
            }
        }

        let mounts = Rc::new(Cell::new(0));
        let mut tree = ComponentTree::new(Parent {
            observed: 0,
            mounts: mounts.clone(),
        });
        assert_eq!(mounts.get(), 1);
        tree.dispatch(Event::Click {
            target: NodeId::from_key("child-action"),
        });
        let snapshot = TreeSnapshot::from_node(&tree.view()).unwrap();
        assert_eq!(
            snapshot
                .get(NodeId::from_key("parent-observed"))
                .unwrap()
                .text
                .as_deref(),
            Some("Observed 1")
        );
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
        let values = sink
            .borrow_mut()
            .drain(..)
            .map(|queued| *queued.message.downcast::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values, vec![1, 2]);
    }

    #[test]
    fn layout_respects_fixed_height_and_gap() {
        let root = Node::column_with_layout(
            "root",
            [
                Node::label_with_layout("a", "A", LayoutStyle::new().height(SizeMode::Fixed(20))),
                Node::label_with_layout("b", "B", LayoutStyle::new().height(SizeMode::Fixed(30))),
            ],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(10)).gap(5),
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
                accessibility: AccessibilityInfo::new(AccessibilityRole::Label)
                    .name("B")
                    .focusable(false),
                style_override: VisualStyle::default(),
                visual_style: VisualStyle::default(),
                disabled: false,
            })]
        );
    }

    #[test]
    fn layout_is_deterministic_and_respects_child_order() {
        let root = Node::column("root", [label("first", "First"), label("second", "Second")]);
        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let engine = LayoutEngine;
        let layout = engine.layout(&snapshot, Size::new(400, 200));

        assert!(layout[&NodeId::from_key("first")].y < layout[&NodeId::from_key("second")].y);
    }

    #[test]
    fn nested_layout_coordinates_are_relative_to_parent() {
        let root = Node::column("root", [Node::column("nested", [label("child", "Child")])]);
        let snapshot = TreeSnapshot::from_node(&root).unwrap();
        let engine = LayoutEngine;
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
                Constraints::new()
                    .with_min_width(100)
                    .with_max_width(120)
                    .with_min_height(20)
                    .with_max_height(40),
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
            [Node::label_with_layout(
                "wrapped",
                "abcdefghij",
                LayoutStyle::new().width(SizeMode::Fixed(50)),
            )],
        ))
        .unwrap();
        let layout = LayoutEngine::new().layout_with(&snapshot, Size::new(200, 100), &WrapMeasurer);
        let rect = layout.get(&NodeId::from_key("wrapped")).unwrap();
        assert_eq!(rect.width, 50);
        assert_eq!(rect.height, 40);
    }

    #[test]
    fn text_update_invalidates_layout() {
        let old = TreeSnapshot::from_node(&Node::column("root", [Node::label("a", "A")])).unwrap();
        let new = TreeSnapshot::from_node(&Node::column(
            "root",
            [Node::label("a", "A much longer label")],
        ))
        .unwrap();
        let diff = TreeDiff::between(&old, &new);
        assert!(diff.invalidates_layout());
    }

    #[test]
    fn diff_detects_insert_remove_and_move() {
        let old =
            TreeSnapshot::from_node(&Node::column("root", [label("a", "A"), label("b", "B")]))
                .unwrap();
        let new =
            TreeSnapshot::from_node(&Node::column("root", [label("b", "B"), label("c", "C")]))
                .unwrap();

        let diff = TreeDiff::between(&old, &new);

        assert!(
            diff.operations.contains(&TreeOp::Remove(TreeNode {
                id: NodeId::from_key("a"),
                kind: NodeKind::Label,
                parent: Some(NodeId::from_key("root")),
                index: 0,
                text: Some("A".into()),
                layout: LayoutStyle::default(),
                column_style: None,
                row_style: None,
                accessibility: AccessibilityInfo::new(AccessibilityRole::Label)
                    .name("A")
                    .focusable(false),
                style_override: VisualStyle::default(),
                visual_style: VisualStyle::default(),
                disabled: false,
            }))
        );
        assert!(
            diff.operations.contains(&TreeOp::Insert(TreeNode {
                id: NodeId::from_key("c"),
                kind: NodeKind::Label,
                parent: Some(NodeId::from_key("root")),
                index: 1,
                text: Some("C".into()),
                layout: LayoutStyle::default(),
                column_style: None,
                row_style: None,
                accessibility: AccessibilityInfo::new(AccessibilityRole::Label)
                    .name("C")
                    .focusable(false),
                style_override: VisualStyle::default(),
                visual_style: VisualStyle::default(),
                disabled: false,
            }))
        );
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
        assert_eq!(
            scrolled.content_sizes[&NodeId::from_key("root")].height,
            first_content_height
        );
        assert_eq!(
            scrolled.scroll_ranges[&NodeId::from_key("root")].height,
            first_scroll_range
        );
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
        assert_eq!(
            snapshot.get(id).and_then(|node| node.text.as_deref()),
            Some("Alice")
        );

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

        let event = Event::Click {
            target: NodeId::from_key("child"),
        };
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
        assert!(!host.update(Event::Click {
            target: NodeId::from_key("outside")
        }));
        assert_eq!(host.component().updates, 0);

        assert!(host.update(Event::Click {
            target: NodeId::from_key("child")
        }));
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
        host.update(Event::Click {
            target: NodeId::from_key("child"),
        });

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

        enum Message {
            Done,
        }

        struct Child {
            show: bool,
            task: Option<TaskHandle>,
        }

        impl Component for Child {
            type Props = Props;
            type Message = Message;

            fn new(_: Props) -> Self {
                Self {
                    show: false,
                    task: None,
                }
            }
            fn props(&self) -> &Props {
                static PROPS: Props = Props;
                &PROPS
            }
            fn set_props(&mut self, _: Props) {}
            fn view(&self) -> Node {
                Node::label("child", if self.show { "done" } else { "idle" })
            }
            fn update(&mut self, _: Event) {}
            fn message(&mut self, _: Message) {
                self.show = true;
                self.task = None;
            }

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

        struct Parent {
            show: bool,
        }
        impl Component for Parent {
            type Props = ();
            type Message = ();
            fn new(_: ()) -> Self {
                Self { show: true }
            }
            fn props(&self) -> &() {
                static PROPS: () = ();
                &PROPS
            }
            fn set_props(&mut self, _: ()) {}
            fn view(&self) -> Node {
                Node::label("parent", if self.show { "on" } else { "off" })
            }
            fn update(&mut self, event: Event) {
                if event.target() == Some(NodeId::from_key("toggle")) {
                    self.show = !self.show;
                }
            }
            fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
                if self.show {
                    context.child::<Child>("child")
                } else {
                    Node::label("empty", "empty")
                }
            }
        }

        let mut tree = ComponentTree::new(Parent::new(()));
        let child_id = ComponentId::child(ComponentId::ROOT, "child");
        assert!(tree.components.contains_key(&child_id));
        assert_eq!(
            tree.components
                .get(&child_id)
                .unwrap()
                .task_scope
                .task_count(),
            1
        );

        // Render without the child. Its scope is cancelled as part of unmount.
        if let Some(root) = tree.components.get_mut(&ComponentId::ROOT) {
            root.component.update(Event::Click {
                target: NodeId::from_key("toggle"),
            });
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

        enum AsyncMessage {
            Done,
        }

        struct AsyncComponent {
            completed: bool,
            task: Option<TaskHandle>,
        }

        impl Component for AsyncComponent {
            type Props = AsyncProps;
            type Message = AsyncMessage;

            fn new(_: Self::Props) -> Self {
                Self {
                    completed: false,
                    task: None,
                }
            }
            fn props(&self) -> &Self::Props {
                static PROPS: AsyncProps = AsyncProps;
                &PROPS
            }
            fn set_props(&mut self, _: Self::Props) {}
            fn view(&self) -> Node {
                Node::label("status", if self.completed { "done" } else { "idle" })
            }
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
                if completed {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(completed);
    }

    #[test]
    fn effects_rerun_only_for_changed_dependencies_and_clean_up_on_unmount() {
        #[derive(Clone, PartialEq)]
        struct EffectProps {
            dependency: u32,
            runs: Rc<Cell<u32>>,
            cleanups: Rc<Cell<u32>>,
        }

        struct EffectChild {
            props: EffectProps,
        }
        impl Component for EffectChild {
            type Props = EffectProps;
            type Message = ();
            fn new(props: Self::Props) -> Self {
                Self { props }
            }
            fn props(&self) -> &Self::Props {
                &self.props
            }
            fn set_props(&mut self, props: Self::Props) {
                self.props = props;
            }
            fn view(&self) -> Node {
                Node::label("effect-child", "child")
            }
            fn update(&mut self, _: Event) {}
            fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
                let runs = self.props.runs.clone();
                let cleanups = self.props.cleanups.clone();
                context.effect("observe-dependency", self.props.dependency, move |_| {
                    runs.set(runs.get() + 1);
                    Box::new(move || cleanups.set(cleanups.get() + 1))
                });
                self.view()
            }
        }

        struct Parent {
            show: bool,
            dependency: u32,
            runs: Rc<Cell<u32>>,
            cleanups: Rc<Cell<u32>>,
        }
        impl Component for Parent {
            type Props = ();
            type Message = ();
            fn new(_: ()) -> Self {
                Self {
                    show: true,
                    dependency: 1,
                    runs: Rc::new(Cell::new(0)),
                    cleanups: Rc::new(Cell::new(0)),
                }
            }
            fn props(&self) -> &() {
                static PROPS: () = ();
                &PROPS
            }
            fn set_props(&mut self, _: ()) {}
            fn view(&self) -> Node {
                Node::label("effect-parent", "parent")
            }
            fn update(&mut self, event: Event) {
                match event.target() {
                    Some(id) if id == NodeId::from_key("change-effect") => self.dependency += 1,
                    Some(id) if id == NodeId::from_key("remove-effect") => self.show = false,
                    _ => {}
                }
            }
            fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
                if self.show {
                    context.child_with_props(
                        "effect-child",
                        EffectProps {
                            dependency: self.dependency,
                            runs: self.runs.clone(),
                            cleanups: self.cleanups.clone(),
                        },
                        EffectChild::new,
                    )
                } else {
                    Node::label("effect-removed", "removed")
                }
            }
        }

        let runs = Rc::new(Cell::new(0));
        let cleanups = Rc::new(Cell::new(0));
        let mut tree = ComponentTree::new(Parent {
            show: true,
            dependency: 1,
            runs: runs.clone(),
            cleanups: cleanups.clone(),
        });
        assert_eq!(runs.get(), 1);
        assert_eq!(cleanups.get(), 0);

        tree.render();
        assert_eq!(runs.get(), 1, "unchanged dependencies retain the effect");
        assert_eq!(cleanups.get(), 0);

        tree.dispatch(Event::Click {
            target: NodeId::from_key("change-effect"),
        });
        assert_eq!(runs.get(), 2);
        assert_eq!(cleanups.get(), 1);

        tree.dispatch(Event::Click {
            target: NodeId::from_key("remove-effect"),
        });
        assert_eq!(cleanups.get(), 2);
    }

    #[test]
    fn memory_services_are_async_compatible_and_deterministic() {
        let storage = MemoryStorage::default();
        block_on(storage.set("key".into(), b"value".to_vec())).unwrap();
        assert_eq!(
            block_on(storage.get("key".into())).unwrap(),
            Some(b"value".to_vec())
        );
        block_on(storage.remove("key".into())).unwrap();
        assert_eq!(block_on(storage.get("key".into())).unwrap(), None);

        let clipboard = MemoryClipboard::default();
        block_on(clipboard.write_text("copied".into())).unwrap();
        assert_eq!(
            block_on(clipboard.read_text()).unwrap().as_deref(),
            Some("copied")
        );
    }

    #[test]
    fn theme_merges_node_overrides_and_state_styles() {
        let mut theme = Theme::default();
        theme.button.focused = Some(VisualStyle::new().background(Color::rgb(1, 2, 3)));
        let override_style = VisualStyle::new().foreground(Color::rgb(4, 5, 6));
        let resolved = theme.resolve(NodeKind::Button, ControlState::Focused, &override_style);
        assert_eq!(resolved.background, Some(Color::rgb(1, 2, 3)));
        assert_eq!(resolved.foreground, Some(Color::rgb(4, 5, 6)));

        let capabilities =
            PlatformCapabilities::new([Capability::Clipboard, Capability::MultipleWindows]);
        assert!(capabilities.supports(Capability::Clipboard));
        assert!(!capabilities.supports(Capability::Camera));
    }

    #[test]
    fn application_keeps_window_component_roots_independent() {
        #[derive(Clone)]
        struct WindowComponent {
            label: &'static str,
        }
        impl Component for WindowComponent {
            type Props = &'static str;
            type Message = ();
            fn new(props: Self::Props) -> Self {
                Self { label: props }
            }
            fn props(&self) -> &Self::Props {
                &self.label
            }
            fn set_props(&mut self, props: Self::Props) {
                self.label = props;
            }
            fn view(&self) -> Node {
                Node::label(self.label, self.label)
            }
            fn update(&mut self, _: Event) {}
        }

        let mut application = Application::new(
            WindowComponent::new("primary-root"),
            Window::new("Primary", Size::new(320, 240)),
        );
        let second = application.open_window(
            WindowComponent::new("secondary-root"),
            Window::new("Secondary", Size::new(200, 160)),
            Some(WindowId::PRIMARY),
        );

        assert_eq!(application.window_ids(), vec![WindowId::PRIMARY, second]);
        assert!(
            application
                .view_for(second)
                .unwrap()
                .contains_id(NodeId::from_key("secondary-root"))
        );
        assert!(
            application
                .view()
                .contains_id(NodeId::from_key("primary-root"))
        );
        assert_eq!(
            application.window_state(second).unwrap().modal_parent,
            Some(WindowId::PRIMARY)
        );
        assert!(application.close_window(second));
        assert!(!application.close_window(WindowId::PRIMARY));
    }

    #[test]
    fn window_lifecycle_events_update_only_their_own_window_state() {
        #[derive(Clone)]
        struct WindowComponent;
        impl Component for WindowComponent {
            type Props = ();
            type Message = ();
            fn new(_: Self::Props) -> Self {
                Self
            }
            fn props(&self) -> &Self::Props {
                static PROPS: () = ();
                &PROPS
            }
            fn set_props(&mut self, _: Self::Props) {}
            fn view(&self) -> Node {
                Node::label("window", "window")
            }
            fn update(&mut self, _: Event) {}
        }

        let mut application =
            Application::new(WindowComponent, Window::new("Primary", Size::new(320, 240)));
        let second = application.open_window(
            WindowComponent,
            Window::new("Secondary", Size::new(200, 160)),
            None,
        );

        assert!(application.dispatch_to_window(
            second,
            Event::WindowResized {
                window: second,
                size: Size::new(640, 480)
            },
        ));
        assert!(application.dispatch_to_window(
            second,
            Event::WindowMoved {
                window: second,
                position: Point::new(24, 36)
            },
        ));
        assert!(application.dispatch_to_window(
            second,
            Event::WindowStateChanged {
                window: second,
                state: WindowPresentation::Maximized
            },
        ));

        let state = application.window_state(second).unwrap();
        assert_eq!(state.size, Size::new(640, 480));
        assert_eq!(state.position, Point::new(24, 36));
        assert_eq!(state.presentation, WindowPresentation::Maximized);
        assert!(state.visible);
        assert_eq!(
            application.window_state(WindowId::PRIMARY).unwrap().size,
            Size::new(320, 240)
        );

        assert!(!application.dispatch_to_window(
            second,
            Event::WindowMoved {
                window: WindowId::PRIMARY,
                position: Point::new(1, 1)
            },
        ));
    }

    #[test]
    fn tree_diff_emits_update_when_only_visual_style_changes() {
        let previous = TreeSnapshot::from_node(&Node::label("status", "Ready")).unwrap();
        let next = TreeSnapshot::from_node(
            &Node::label("status", "Ready")
                .with_style(VisualStyle::new().foreground(Color::rgb(1, 2, 3))),
        )
        .unwrap();

        assert!(matches!(
            TreeDiff::between(&previous, &next).operations.as_slice(),
            [TreeOp::Update(node)] if node.id == NodeId::from_key("status")
        ));
    }

    #[test]
    fn disabled_flag_round_trips_into_tree_snapshot() {
        let root = Node::column(
            "root",
            [
                Node::button("save", "Save").disabled(true),
                label("hint", "Hint"),
            ],
        );
        let snapshot = TreeSnapshot::from_node(&root).unwrap();

        assert!(snapshot.get(NodeId::from_key("save")).unwrap().disabled);
        assert!(!snapshot.get(NodeId::from_key("hint")).unwrap().disabled);
    }

    #[test]
    fn diff_updates_an_existing_node_when_its_disabled_state_changes() {
        let enabled = TreeSnapshot::from_node(&Node::button("save", "Save")).unwrap();
        let disabled =
            TreeSnapshot::from_node(&Node::button("save", "Save").disabled(true)).unwrap();

        assert!(matches!(
            TreeDiff::between(&enabled, &disabled).operations.as_slice(),
            [TreeOp::Update(node)] if node.disabled
        ));
    }

    #[test]
    fn from_node_with_theme_resolves_normal_and_disabled_states() {
        let mut theme = Theme::default();
        theme.button.normal = VisualStyle::new().background(Color::rgb(10, 10, 10));
        theme.button.disabled = Some(VisualStyle::new().background(Color::rgb(200, 200, 200)));

        let root = Node::column(
            "root",
            [
                Node::button("enabled", "Go"),
                Node::button("disabled", "Go").disabled(true),
            ],
        );
        let snapshot = TreeSnapshot::from_node_with_theme(&root, &theme).unwrap();

        assert_eq!(
            snapshot
                .get(NodeId::from_key("enabled"))
                .unwrap()
                .visual_style
                .background,
            Some(Color::rgb(10, 10, 10))
        );
        assert_eq!(
            snapshot
                .get(NodeId::from_key("disabled"))
                .unwrap()
                .visual_style
                .background,
            Some(Color::rgb(200, 200, 200))
        );
    }

    #[test]
    fn themed_snapshot_retains_node_override_for_live_platform_states() {
        let override_color = Color::rgb(25, 50, 75);
        let root =
            Node::button("save", "Save").with_style(VisualStyle::new().foreground(override_color));
        let snapshot = TreeSnapshot::from_node_with_theme(&root, &Theme::default()).unwrap();

        assert_eq!(
            snapshot
                .get(NodeId::from_key("save"))
                .unwrap()
                .style_override
                .foreground,
            Some(override_color)
        );
    }

    #[test]
    fn menu_bar_exposes_actions_submenus_and_separators() {
        let menu = MenuBar::new([
            MenuItem::submenu(
                "file",
                "File",
                [
                    MenuItem::action("file.new", "New"),
                    MenuItem::separator(),
                    MenuItem::action("file.exit", "Exit").enabled(false),
                ],
            ),
            MenuItem::action("view.toolbar", "Show Toolbar").checked(true),
        ]);

        assert_eq!(menu.items().len(), 2);
        let file = &menu.items()[0];
        assert!(file.is_submenu());
        assert_eq!(file.children().len(), 3);
        assert!(file.children()[1].is_separator());
        assert!(!file.children()[2].is_enabled());
        assert_eq!(menu.items()[1].is_checked(), Some(true));

        let window = Window::new("Editor", Size::new(640, 480)).with_menu(menu.clone());
        assert_eq!(window.menu(), Some(&menu));
    }

    #[test]
    fn menu_action_event_targets_no_node_and_routes_by_window() {
        let event = Event::MenuAction {
            window: WindowId::PRIMARY,
            item: NodeId::from_key("file.new"),
        };
        assert_eq!(event.target(), None);
        assert_eq!(event_window_id(&event), Some(WindowId::PRIMARY));
    }

    #[derive(Clone, Default)]
    struct WindowRequestDialog;
    impl Component for WindowRequestDialog {
        type Props = ();
        type Message = ();
        fn new(_: Self::Props) -> Self {
            Self
        }
        fn props(&self) -> &Self::Props {
            static PROPS: () = ();
            &PROPS
        }
        fn set_props(&mut self, _: Self::Props) {}
        fn view(&self) -> Node {
            Node::label("dialog", "Dialog")
        }
        fn update(&mut self, _: Event) {}
    }

    #[test]
    fn component_can_request_opening_a_sibling_window_at_runtime() {
        #[derive(Clone, Default)]
        struct Launcher {
            should_open: bool,
        }
        impl Component for Launcher {
            type Props = ();
            type Message = ();
            fn new(_: Self::Props) -> Self {
                Self::default()
            }
            fn props(&self) -> &Self::Props {
                static PROPS: () = ();
                &PROPS
            }
            fn set_props(&mut self, _: Self::Props) {}
            fn view(&self) -> Node {
                Node::label("launcher", "Launcher")
            }
            fn update(&mut self, event: Event) {
                if event.target() == Some(NodeId::from_key("open")) {
                    self.should_open = true;
                }
            }
            fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
                if self.should_open {
                    self.should_open = false;
                    context.windows().open(
                        WindowRequestDialog,
                        Window::new("Dialog", Size::new(200, 120)),
                        Some(WindowId::PRIMARY),
                    );
                }
                self.view()
            }
        }

        let mut application = Application::new(
            Launcher::default(),
            Window::new("Primary", Size::new(320, 240)),
        );
        assert_eq!(application.window_ids(), vec![WindowId::PRIMARY]);

        application.dispatch(Event::Click {
            target: NodeId::from_key("open"),
        });

        let ids = application.window_ids();
        assert_eq!(ids.len(), 2);
        let opened = ids
            .into_iter()
            .find(|id| *id != WindowId::PRIMARY)
            .expect("a second window must have been opened");
        assert!(
            application
                .view_for(opened)
                .unwrap()
                .contains_id(NodeId::from_key("dialog"))
        );
        assert_eq!(
            application.window_state(opened).unwrap().modal_parent,
            Some(WindowId::PRIMARY)
        );
    }

    #[test]
    fn initial_and_explicit_renders_apply_deferred_window_requests() {
        #[derive(Clone, Default)]
        struct Launcher {
            open_on_render: bool,
        }
        impl Component for Launcher {
            type Props = bool;
            type Message = ();
            fn new(open_on_render: Self::Props) -> Self {
                Self { open_on_render }
            }
            fn props(&self) -> &Self::Props {
                &self.open_on_render
            }
            fn set_props(&mut self, open_on_render: Self::Props) {
                self.open_on_render = open_on_render;
            }
            fn view(&self) -> Node {
                Node::label("initial-launcher", "Launcher")
            }
            fn update(&mut self, _: Event) {}
            fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
                if self.open_on_render {
                    self.open_on_render = false;
                    context.windows().open(
                        WindowRequestDialog,
                        Window::new("Initial dialog", Size::new(200, 120)),
                        Some(WindowId::PRIMARY),
                    );
                }
                self.view()
            }
        }

        let mut application = Application::new(
            Launcher::new(true),
            Window::new("Primary", Size::new(320, 240)),
        );
        assert_eq!(application.window_ids().len(), 2);

        // Opening a component directly also completes its initial render
        // transaction, rather than waiting for an event in that window.
        let direct = application.open_window(
            Launcher::new(true),
            Window::new("Direct launcher", Size::new(320, 240)),
            None,
        );
        assert!(application.window_ids().len() >= 4);
        assert!(application.window_state(direct).is_some());
    }

    #[test]
    fn closing_a_modal_parent_closes_its_modal_descendants() {
        let mut application = Application::new(
            WindowRequestDialog,
            Window::new("Primary", Size::new(320, 240)),
        );
        let parent = application.open_window(
            WindowRequestDialog,
            Window::new("Parent", Size::new(200, 160)),
            None,
        );
        let child = application.open_window(
            WindowRequestDialog,
            Window::new("Child", Size::new(160, 120)),
            Some(parent),
        );
        let grandchild = application.open_window(
            WindowRequestDialog,
            Window::new("Grandchild", Size::new(120, 80)),
            Some(child),
        );

        assert!(application.close_window(parent));
        assert!(application.window_state(parent).is_none());
        assert!(application.window_state(child).is_none());
        assert!(application.window_state(grandchild).is_none());
        assert_eq!(application.window_ids(), vec![WindowId::PRIMARY]);
    }

    #[test]
    fn component_can_request_closing_a_window_at_runtime() {
        #[derive(Clone)]
        struct Dismissible {
            target: WindowId,
        }
        impl Component for Dismissible {
            type Props = WindowId;
            type Message = ();
            fn new(props: Self::Props) -> Self {
                Self { target: props }
            }
            fn props(&self) -> &Self::Props {
                &self.target
            }
            fn set_props(&mut self, props: Self::Props) {
                self.target = props;
            }
            fn view(&self) -> Node {
                Node::label("dismissible", "Dismissible")
            }
            fn update(&mut self, _: Event) {}
            fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
                context.windows().close(self.target);
                self.view()
            }
        }

        let mut application = Application::new(
            WindowRequestDialog,
            Window::new("Primary", Size::new(320, 240)),
        );
        let second = application.open_window(
            WindowRequestDialog,
            Window::new("Secondary", Size::new(200, 160)),
            None,
        );
        let closer = application.open_window(
            Dismissible::new(second),
            Window::new("Closer", Size::new(100, 100)),
            None,
        );
        // Initial rendering is a complete transaction: the close request is
        // applied without requiring an unrelated later event in `closer`.
        assert_eq!(application.window_ids().len(), 2);

        let ids = application.window_ids();
        assert!(!ids.contains(&second));
        assert!(ids.contains(&closer));
        assert!(ids.contains(&WindowId::PRIMARY));
    }
}
