#![cfg_attr(windows, windows_subsystem = "windows")]

use framework_core::{Application, Component, Platform, Size, Window};
use framework_windows::WindowsPlatform;
use gallery::Gallery;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut application =
        Application::new(Gallery::new(()), Window::new("Rust Native gallery", Size::new(900, 720)));
    application.set_theme(gallery::theme());
    WindowsPlatform::new().run(&mut application)?;
    Ok(())
}
