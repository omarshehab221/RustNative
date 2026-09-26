#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
use std::sync::Arc;

use framework_core::product::Flags;
use framework_core::{Application, Component, Platform, Services, Size, Window};
use framework_windows::WindowsPlatform;
use product_services::Product;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Remote configuration would be fetched with `Flags::refresh`; locally,
    // `RUSTNATIVE_FLAGS=compact-layout=true` turns the feature on.
    let services = Services::default().with_flags(Flags::new());
    #[cfg(windows)]
    let services = services
        .with_secure_storage(Arc::new(framework_windows::WindowsSecureStorage::new(
            "dev.rustnative.product-services",
        )))
        .with_push(Arc::new(framework_windows::WindowsPush))
        .with_commerce(Arc::new(framework_windows::WindowsStore));
    let mut application = Application::with_services(
        Product::new(()),
        Window::new("Product services", Size::new(480, 320)),
        services,
    );
    WindowsPlatform::new().run(&mut application)?;
    Ok(())
}
