//! The conformance suites (`PLAN.md` Milestone 41), starting with the one
//! Milestone 53 creates: **syntax equivalence** — every node kind and every
//! modifier written three ways, with the builder syntax, with markup in
//! `rsx!`, and with markup in a `.rsx` file, and asserted to produce equal
//! `Node` values (`tests/syntax_equivalence.rs`).
//!
//! A case added to one spelling and not the others fails the suite, which
//! is the only mechanism that keeps two authoring surfaces equal over
//! years (2.9).
#![deny(missing_docs)]

pub mod builder_cases;
pub mod macro_cases;
pub mod syntax;

framework_core::rsx_mod!(
    /// The `.rsx` spelling of every case (`src/markup_file.rsx`).
    pub markup_file
);
