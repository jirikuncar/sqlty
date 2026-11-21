//! Public-API tests of the crate, one module per source module.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test fails by panicking"
)]

mod breaking;
mod calls;
mod check;
mod checker;
mod common;
mod config;
mod drivers;
mod fixer;
mod frozen;
mod index;
mod locate;
mod migrations;
mod module;
mod params;
mod parse;
mod project;
mod project_glob;
mod project_modules;
mod project_route;
mod project_walk;
mod queries;
mod report;
mod rewriter;
mod rows_json_shape;
mod rules;
mod scopes;
mod suppress;
mod templates;
mod types_compat;
mod types_stdlib;
mod types_ty;
mod unparse;
