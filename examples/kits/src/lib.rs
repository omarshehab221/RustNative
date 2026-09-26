//! The feature kits (`PLAN.md` Milestone 52, `C57-3`), compiled and
//! tested exactly as `rustnative generate kit` writes them: these modules
//! are the templates themselves.

#[path = "../../../crates/rustnative/templates/kits/auth.rs"]
pub mod auth;

#[path = "../../../crates/rustnative/templates/kits/admin.rs"]
pub mod admin;

#[path = "../../../crates/rustnative/templates/kits/commerce.rs"]
pub mod commerce;
