#![cfg_attr(windows, windows_subsystem = "windows")]

use framework_core::{
    AccessibilityInfo, AccessibilityRole, Alignment, Application, Callback, ColumnStyle, Component,
    ComponentContext, EdgeInsets, Event, LayoutStyle, MenuBar, MenuItem, Node, NodeId, Overflow,
    PanicPolicy, Platform, RowStyle, Size, SizeMode, TaskHandle, Window,
};
use framework_windows::WindowsPlatform;

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppMessage {
    ChildCountChanged(u32),
    ChildNameChanged(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CounterPanelProps {
    title: String,
    on_message: Callback<AppMessage>,
}

enum CounterMessage {
    AsyncCompleted,
}

struct CounterPanel {
    props: CounterPanelProps,
    count: u32,
    name: String,
    mounted_count: u32,
    unmounted_count: u32,
    props_update_count: u32,
    async_task: Option<TaskHandle>,
    async_requested: bool,
    async_status: String,
}

impl Component for CounterPanel {
    type Props = CounterPanelProps;
    type Message = CounterMessage;

    fn new(props: Self::Props) -> Self {
        Self {
            props,
            count: 0,
            name: String::new(),
            mounted_count: 0,
            unmounted_count: 0,
            props_update_count: 0,
            async_task: None,
            async_requested: false,
            async_status: "Idle".to_owned(),
        }
    }

    fn props(&self) -> &Self::Props {
        &self.props
    }

    fn set_props(&mut self, props: Self::Props) {
        self.props = props;
    }

    fn view(&self) -> Node {
        Node::column_with_layout(
            "counter-panel",
            [
                Node::label_with_layout(
                    "title",
                    self.props.title.clone(),
                    LayoutStyle::new().height(SizeMode::Fixed(32)),
                ),
                Node::label_with_layout(
                    "counter",
                    format!("Count: {}", self.count),
                    LayoutStyle::new().height(SizeMode::Fixed(32)),
                ),
                Node::text_input_with_layout(
                    "name",
                    self.name.clone(),
                    LayoutStyle::new()
                        .width(SizeMode::Fixed(480))
                        .height(SizeMode::Fixed(32))
                        .align_self(Alignment::Center),
                ),
                Node::row_with_layout(
                    "controls",
                    [
                        Node::button_with_layout(
                            "increment",
                            "Increment",
                            LayoutStyle::new().width(SizeMode::Auto).height(SizeMode::Fixed(36)),
                        )
                        // Demonstrates `disabled`: past 5, the button is
                        // realized with `EnableWindow(hwnd, 0)` and the
                        // theme's `ControlState::Disabled` style.
                        .disabled(self.count >= 5),
                        Node::button_with_layout(
                            "async-task",
                            if self.async_task.is_some() { "Cancel Async" } else { "Run Async" },
                            LayoutStyle::new().width(SizeMode::Auto).height(SizeMode::Fixed(36)),
                        )
                        // Demonstrates the accessibility bridge. This
                        // button's visible label changes as the task runs,
                        // so its *announced* name is pinned to something
                        // stable and unambiguous instead — realized on
                        // Windows through `IAccPropServices`, which
                        // overrides the accessible name of the standard
                        // control without replacing the control.
                        //
                        // Every node already carries a sensible default
                        // (a button is `Role::Button` and focusable); this
                        // is the case where the default is not enough.
                        .with_accessibility(
                            AccessibilityInfo::new(AccessibilityRole::Button)
                                .name("Run or cancel the background task")
                                .description(
                                    "Starts a two-second task, or cancels it if one is running",
                                )
                                .focusable(true),
                        ),
                        Node::label_with_layout(
                            "async-status",
                            format!("Async: {}", self.async_status),
                            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Auto),
                        ),
                        Node::label_with_layout(
                            "lifecycle",
                            format!(
                                "Mounted: {} | Unmounted: {} | Props updated: {}",
                                self.mounted_count, self.unmounted_count, self.props_update_count
                            ),
                            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Auto),
                        ),
                    ],
                    LayoutStyle::new().width(SizeMode::Fixed(520)).align_self(Alignment::Center),
                    RowStyle::new()
                        .padding(EdgeInsets::symmetric(12, 20))
                        .gap(12)
                        .align_items(Alignment::Center),
                ),
                Node::column_with_layout(
                    "items",
                    (0..24).map(|index| {
                        Node::label_with_layout(
                            format!("item-{index}"),
                            format!("Scrollable item {index}"),
                            LayoutStyle::new().height(SizeMode::Fixed(28)),
                        )
                    }),
                    LayoutStyle::new()
                        .width(SizeMode::Fixed(480))
                        .height(SizeMode::Fixed(180))
                        .align_self(Alignment::Center),
                    ColumnStyle::new()
                        .padding(EdgeInsets::all(12))
                        .gap(6)
                        .overflow(Overflow::Scroll),
                ),
            ],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(24)).gap(16).align_items(Alignment::Center),
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("increment") => {
                self.count += 1;
                self.props.on_message.send(AppMessage::ChildCountChanged(self.count));
            }
            Event::Click { target } if target == NodeId::from_key("async-task") => {
                if let Some(task) = &self.async_task {
                    task.cancel();
                    self.async_task = None;
                    "Cancelled".clone_into(&mut self.async_status);
                } else {
                    self.async_requested = true;
                    "Running".clone_into(&mut self.async_status);
                }
            }
            Event::TextChanged { target, value } if target == NodeId::from_key("name") => {
                self.name.clone_from(&value);
                self.props.on_message.send(AppMessage::ChildNameChanged(value));
            }
            // The explicit list below is deliberate documentation, not dead
            // code: it names every event kind this component chooses not to
            // react to, so a reader can see at a glance what was considered
            // rather than just assuming an oversight. `#[allow]`d rather
            // than collapsed into the wildcard alone, since collapsing it
            // would lose exactly that value for a lint whose concern
            // (identical arm bodies) doesn't apply to documentation intent.
            #[allow(clippy::match_same_arms)]
            Event::KeyDown { .. }
            | Event::TextInput { .. }
            | Event::TextChanged { .. }
            | Event::Click { .. }
            | Event::FocusGained { .. }
            | Event::FocusLost { .. }
            | Event::WindowResized { .. }
            | Event::WindowMoved { .. }
            | Event::WindowCloseRequested { .. }
            | Event::WindowStateChanged { .. }
            | Event::MenuAction { .. } => {}
            // `Event` is `#[non_exhaustive]` (framework-core standards audit
            // P2.4): a future new variant lands here by default rather than
            // failing to build every downstream crate.
            _ => {}
        }
    }

    fn message(&mut self, message: Self::Message) {
        match message {
            CounterMessage::AsyncCompleted => {
                self.async_task = None;
                self.async_requested = false;
                "Completed".clone_into(&mut self.async_status);
            }
        }
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        if self.async_requested && self.async_task.is_none() {
            self.async_requested = false;
            let delay = context.sleep(std::time::Duration::from_secs(2));
            let task = context.spawn(async move {
                delay.await;
                CounterMessage::AsyncCompleted
            });
            self.async_task = Some(task);
        }
        self.view()
    }

    fn props_changed(&mut self) {
        self.props_update_count += 1;
    }

    fn mounted(&mut self) {
        self.mounted_count += 1;
    }

    fn unmounted(&mut self) {
        // Async tasks are owned by the framework-managed component scope.
        // The scope is cancelled automatically when this component unmounts.
        self.async_task = None;
        self.unmounted_count += 1;
    }
}

struct AppShell {
    panel_visible: bool,
    title_index: usize,
    child_count: u32,
    child_name: String,
    settings_requested: bool,
    settings_opened_count: u32,
}

/// A separate root used to exercise the native multi-window host. It has no
/// shared component state with `AppShell`; the framework owns its lifetime and
/// scheduler independently.
struct AuxiliaryWindow;

impl Component for AuxiliaryWindow {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        Self
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::column_with_layout(
            "auxiliary-root",
            [Node::label_with_layout(
                "auxiliary-label",
                "Independent native window",
                LayoutStyle::new().height(SizeMode::Fixed(36)),
            )],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(24)),
        )
    }

    fn update(&mut self, _: Event) {}
}

/// Opened dynamically at runtime from the "File \u{2192} New Settings Window"
/// menu action (see `AppShell::render`) rather than up front in `main`, to
/// exercise `ComponentContext::windows()`.
struct SettingsWindow {
    opened_at_click: u32,
}

impl Component for SettingsWindow {
    type Props = u32;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { opened_at_click: props }
    }

    fn props(&self) -> &Self::Props {
        &self.opened_at_click
    }

    fn set_props(&mut self, props: Self::Props) {
        self.opened_at_click = props;
    }

    fn view(&self) -> Node {
        Node::column_with_layout(
            "settings-root",
            [Node::label_with_layout(
                "settings-label",
                format!("Opened from the menu (request #{})", self.opened_at_click),
                LayoutStyle::new().height(SizeMode::Fixed(36)),
            )],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(24)),
        )
    }

    fn update(&mut self, _: Event) {}
}

impl AppShell {
    fn new() -> Self {
        Self {
            panel_visible: true,
            title_index: 0,
            child_count: 0,
            child_name: String::new(),
            settings_requested: false,
            settings_opened_count: 0,
        }
    }

    fn panel_title(&self) -> &'static str {
        ["Counter Panel — Parent-owned title", "Counter Panel — Props changed without remounting"]
            [self.title_index]
    }
}

impl Component for AppShell {
    type Props = ();
    type Message = AppMessage;

    fn new((): Self::Props) -> Self {
        Self::new()
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::label("app-shell-placeholder", "Managed component tree")
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        if self.settings_requested {
            self.settings_requested = false;
            self.settings_opened_count += 1;
            // Deferred: applied by `Application` once this render finishes
            // (see `ComponentContext::windows`), then realized as a new
            // native top-level window by the platform backend.
            context.windows().open(
                SettingsWindow::new(self.settings_opened_count),
                Window::new("Rust Native UI — Settings", Size::new(360, 160)),
                None,
            );
        }

        let panel = if self.panel_visible {
            Some(context.child_with_props(
                "counter-panel",
                CounterPanelProps {
                    title: self.panel_title().to_owned(),
                    on_message: context.callback(),
                },
                CounterPanel::new,
            ))
        } else {
            None
        };

        Node::column_with_layout(
            "app-shell",
            [
                Node::row_with_layout(
                    "lifecycle-controls",
                    [
                        Node::button_with_layout(
                            "toggle-panel",
                            if self.panel_visible { "Unmount Panel" } else { "Mount Panel" },
                            LayoutStyle::new().height(SizeMode::Fixed(36)),
                        ),
                        Node::button_with_layout(
                            "change-title",
                            "Change Child Props",
                            LayoutStyle::new().height(SizeMode::Fixed(36)),
                        ),
                    ],
                    LayoutStyle::new().width(SizeMode::Fixed(520)).align_self(Alignment::Center),
                    RowStyle::new().gap(12).align_items(Alignment::Center),
                ),
                Node::label_with_layout(
                    "child-status",
                    format!("Child count: {} | Name: {}", self.child_count, self.child_name),
                    LayoutStyle::new().height(SizeMode::Fixed(32)),
                ),
                panel.unwrap_or_else(|| {
                    Node::label_with_layout(
                        "empty-panel",
                        "The panel is currently unmounted.",
                        LayoutStyle::new()
                            .height(SizeMode::Fixed(40))
                            .align_self(Alignment::Center),
                    )
                }),
            ],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(24)).gap(16).align_items(Alignment::Center),
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } => {
                if target == NodeId::from_key("toggle-panel") {
                    self.panel_visible = !self.panel_visible;
                } else if target == NodeId::from_key("change-title") {
                    self.title_index = (self.title_index + 1) % 2;
                }
            }
            Event::MenuAction { item, .. } => {
                if item == NodeId::from_key("view.toggle-panel") {
                    self.panel_visible = !self.panel_visible;
                } else if item == NodeId::from_key("file.new-settings-window") {
                    self.settings_requested = true;
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, message: Self::Message) {
        match message {
            AppMessage::ChildCountChanged(count) => self.child_count = count,
            AppMessage::ChildNameChanged(name) => self.child_name = name,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let menu = MenuBar::new([
        MenuItem::submenu(
            "file",
            "File",
            [MenuItem::action("file.new-settings-window", "New Settings Window")],
        ),
        MenuItem::submenu("view", "View", [MenuItem::action("view.toggle-panel", "Toggle Panel")]),
    ]);

    let mut application = Application::new(
        AppShell::new(),
        Window::new("Rust Native UI", Size::new(640, 360)).with_menu(menu),
    );
    // A component panic is caught at the Win32 callback boundary regardless
    // (unwinding across `extern "system"` is undefined behavior, so that
    // part is not a policy). What *is* a policy is what happens next, and
    // the framework defaults to ending the application because that is the
    // only response that cannot keep running on state a panic already
    // disproved.
    //
    // This example opts into closing just the offending window instead,
    // which is the right trade for a multi-window app where the other
    // windows hold independent state — see `framework_core::panic` for the
    // full reasoning.
    application.set_panic_policy(PanicPolicy::CloseWindow);
    application.open_window(
        AuxiliaryWindow,
        Window::new("Rust Native UI — Auxiliary", Size::new(320, 160)),
        None,
    );

    WindowsPlatform::new().run(&mut application)?;

    Ok(())
}
