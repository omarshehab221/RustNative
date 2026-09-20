//! What a new project is made of.
//!
//! Small on purpose: a window, a button, and the three files that make it a
//! project (`Cargo.toml`, `rf.toml`, `.gitignore`). Everything a person is
//! likely to change is in `src/main.rs`, and everything the tooling reads
//! is in `rf.toml`.

/// The generated `src/main.rs`.
pub const MAIN_RS: &str = r#"#![cfg_attr(windows, windows_subsystem = "windows")]

use framework_core::{
    Application, Component, ComponentContext, Event, Node, NodeId, Platform, Size, Window,
};
use framework_windows::WindowsPlatform;

/// The application's root component: state, a view of it, and what events
/// do to it.
struct App {
    clicks: u32,
}

impl Component for App {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        Self { clicks: 0 }
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("greeting", format!("Hello from {APP_NAME}")),
                Node::label("count", format!("Clicked {} times", self.clicks)),
                Node::button("click", "Click me"),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("click")) {
            self.clicks += 1;
        }
    }
}

const APP_NAME: &str = "{{display_name}}";
const APP_ID: &str = "{{app_id}}";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut application = Application::new(
        App::new(()),
        Window::new(APP_NAME, Size::new(480, 320)),
    );
    WindowsPlatform::new().with_app_id(APP_ID).run(&mut application)?;
    Ok(())
}
"#;

/// The generated `Cargo.toml`, with `{{dependencies}}` filled in depending
/// on whether the project is built against a checkout of the framework or
/// against published versions.
pub const CARGO_TOML: &str = r#"[package]
name = "{{name}}"
version = "{{version}}"
edition = "2024"
rust-version = "1.85"
publish = false

[dependencies]
{{dependencies}}
"#;

/// The generated `.gitignore`.
pub const GITIGNORE: &str = "/target\n";

/// The generated `README.md`.
pub const README: &str = r"# {{display_name}}

A [Rust Native](https://github.com/<org>/RustNative) application.

```sh
rf run windows      # build and run
rf build windows    # build only, `--release` for an optimized build
rf test             # run the project's tests
rf doctor           # check the toolchains this machine has
```

`rf.toml` holds what the tooling needs to know about this application: its
identity (used for its saved state and to keep one instance running), the
name people see, and its version.
";
