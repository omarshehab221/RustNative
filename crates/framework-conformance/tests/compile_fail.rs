//! The markup diagnostics, held by a compile-failure suite rather than by
//! inspection (`PLAN.md` Milestone 53): each case must fail to compile with
//! the error its `.stderr` records, spanned to what the developer wrote.
//! `.rsx`-file diagnostics, which are the same errors reported through the
//! source map, are covered by `rustnative`'s CLI tests.

#[test]
fn markup_diagnostics() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
