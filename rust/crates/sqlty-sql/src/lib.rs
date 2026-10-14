//! SQL parsing and type inference.
//!
//! - [`analyzer`]: result-column types of a query
//! - [`diagnostics`]: name diagnostics the analyzer records
//! - [`json`], [`placeholders`]: JSON shapes and placeholder rewriting
//!
//! Crate-internal: `catalog` (the schema: relations, types, functions), `ddl` (splits
//! schema text into statements) and `values` (literal sets carried in Python types).

pub mod analyzer;
pub(crate) mod catalog;
pub(crate) mod ddl;
pub mod diagnostics;
pub mod json;
pub mod placeholders;
pub(crate) mod values;
