//! SQL parsing and type inference: the engine under sqlty, free of any Python.
//!
//! Given a query and, optionally, a schema, the crate says what Python type each result
//! column has and whether it can be NULL, and why a column could not be typed.
//!
//! - [`analyzer`]: result-column types of a query. Start with
//!   [`analyzer::load_schema_with_skipped`] to read `CREATE` statements into a schema, then
//!   [`analyzer::infer_type`] (`tuple[int, str]`), [`analyzer::analyze_select`] (the
//!   columns), [`analyzer::analyze_with_diagnostics`] (the columns and why some are
//!   untyped), [`analyzer::analyze_params`] (the types of the placeholders) and
//!   [`analyzer::expand_star`] (`SELECT *` written out), and [`analyzer::complete`] (what
//!   completes at a cursor in a query).
//! - [`diagnostics`]: the name diagnostics the analyzer records.
//! - [`json`], [`placeholders`]: JSON shapes and placeholder rewriting.
//!
//! Crate-internal: `catalog` (the schema: relations, types, functions), `ddl` (splits
//! schema text into statements) and `values` (literal sets carried in Python types).
//!
//! # Example
//!
//! ```
//! use sqlty_sql::analyzer::{infer_type, load_schema_with_skipped};
//!
//! let (schema, skipped) = load_schema_with_skipped(
//!     "CREATE TABLE users (id INTEGER NOT NULL, name TEXT)",
//!     "postgres",
//! );
//! assert!(skipped.is_empty());
//!
//! let row = infer_type("SELECT id, name FROM users", "postgres", Some(&*schema));
//! assert_eq!(row.as_deref(), Some("tuple[int, str | None]"));
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute), allow(unused_features))]

pub mod analyzer;
pub use analyzer::{SqlDialect as Dialect, UnknownDialect};
pub(crate) mod catalog;
pub(crate) mod ddl;
pub mod diagnostics;
pub mod highlight;
pub mod json;
pub mod migrations;
pub mod placeholders;
pub(crate) mod values;

/// The most columns a table or a result has: MySQL's limit, above PostgreSQL's 1600 and
/// SQLite's default 2000. A wider one is left untyped rather than analyzed in quadratic time.
pub(crate) const MAX_COLUMNS: usize = 4096;

/// A regex written in the source, with at most short names escaped into it: an invalid one
/// is a bug.
#[doc(hidden)]
#[expect(clippy::expect_used, reason = "patterns are written in the source")]
pub fn regex(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).expect("a valid pattern")
}
