//! The behavioural guarantees on the headless reference backend
//! (`docs/guarantees.md`); Windows runs the same suites in
//! `framework-windows`'s `native::guarantees_integration`.

use framework_conformance::host::HeadlessHost;
use framework_conformance::suites;

#[test]
fn typing_renders_only_the_owning_component() {
    suites::typing_renders_only_the_owning_component(&mut HeadlessHost);
}

#[test]
fn scrolling_renders_nothing() {
    suites::scrolling_renders_nothing(&mut HeadlessHost);
}

#[test]
fn one_message_is_one_render() {
    suites::one_message_is_one_render(&mut HeadlessHost);
}

#[test]
fn no_message_after_unmount() {
    suites::no_message_after_unmount(&mut HeadlessHost);
}

#[test]
fn mount_unmount_returns_to_baseline() {
    suites::mount_unmount_returns_to_baseline(&mut HeadlessHost);
}
