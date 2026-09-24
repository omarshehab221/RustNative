//! Procedural macros for Rust Native.
//!
//! A thin shell over `framework-markup`: the macros here parse and lower
//! with that crate and report its errors with their native spans, so a
//! diagnostic inside `rsx!` points at the attribute or element in the `.rs`
//! file with no remapping. Use them through `framework-core`, which
//! re-exports them behind its default-on `markup` feature.
#![deny(missing_docs)]

use proc_macro::TokenStream;

/// The markup syntax, delimited, inside any `.rs` file (`PLAN.md` 2.9).
///
/// Evaluates to a `framework_core::Node`. A tree containing component
/// elements names its context first: `rsx! { in context, <Screen key="s" /> }`.
/// See `framework-core`'s documentation for the grammar, and a `.rsx` file
/// for the same grammar with no wrapper.
#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    match framework_markup::expand(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
