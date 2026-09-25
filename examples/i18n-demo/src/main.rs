#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
use std::sync::Arc;

use framework_core::{Application, Component, Platform, Services, Size, Window};
use framework_windows::WindowsPlatform;
use i18n_demo::{Demo, messages};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let services = Services::default().with_catalogues(messages::catalogues());
    // Numbers, dates, and casing as the host writes them for each locale.
    #[cfg(windows)]
    let services = services.with_locale_service(Arc::new(framework_windows::WindowsLocale));
    let mut application = Application::with_services(
        Demo::new(()),
        Window::new("Rust Native i18n", Size::new(520, 360)),
        services,
    );
    WindowsPlatform::new().run(&mut application)?;
    Ok(())
}
