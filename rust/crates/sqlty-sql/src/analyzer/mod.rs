//! SQL type inference with sqlparser-rs.
//!
//! Rows come from the `SELECT`s of a query, typed across the branches of a set
//! operation, or from the top-level `RETURNING` of INSERT/UPDATE/DELETE/MERGE. Type
//! names are canonicalized (`INTEGER` becomes `INT`, `NUMERIC(10,2)` becomes
//! `DECIMAL(10, 2)`), as `SELECT *` expansion writes them back into queries.

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

mod api;
mod dialect;
mod dml;
mod explain;
mod expr;
mod functions;
mod grouping;
mod hashing;
mod inference;
mod json;
mod literals;
mod memo;
mod merged_star;
mod names;
mod narrowing;
mod nullability;
mod operators;
mod params;
mod pg_record_functions;
mod preprocess;
mod query;
mod references;
mod registry;
mod resolve;
mod respell;
mod scope;
mod sql_type;
mod star_expand;
mod table_functions;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod text_names;
mod typed;

#[cfg(test)]
pub(crate) use api::load_schema;
pub use api::{
    analyze_select, analyze_with_diagnostics, composite_columns, infer_type,
    load_schema_with_overrides, load_schema_with_skipped, parse, parse_error, returns_rows,
    row_type, Parsed,
};
pub(crate) use api::{typed, view_columns};
pub use dialect::{SqlDialect, UnknownDialect};
pub use explain::{explain_select, ColumnRules};
use expr::from_sql_type;
pub use inference::RuleRef;
pub use nullability::Nullability;
pub use params::{analyze_params, Param};
pub(crate) use preprocess::parse_ddl;
pub use query::Column;
pub(crate) use sql_type::canonical_type;
pub use sql_type::python_types;
use sql_type::{decimal_type, python_type, ANY, DECIMAL};
pub use star_expand::{expand_star, result_stars};
use typed::Typed;

pub use crate::catalog::{Catalog as SchemaData, SchemaColumn, TypeOverrides};

/// A loaded schema, shared so callers can cache it across queries.
pub type Schema = Arc<SchemaData>;

impl SchemaColumn {
    fn typed(&self, dialect: SqlDialect) -> Typed {
        match &self.python_type {
            Some(python_type) => Typed {
                python: Some(Rc::from(python_type.as_str())),
                sql: Some(Rc::from(self.sql_type.as_str())),
                values: self.values.clone(),
            },
            None => from_sql_type(&self.sql_type, dialect),
        }
    }

    /// The column's name as a result column: its spelling in the schema, without quotes.
    /// MySQL and SQLite keep the declared case; PostgreSQL folds an unquoted name.
    fn output_name(&self) -> String {
        let ident = self.ident.as_str();
        let unquoted =
            [('"', '"'), ('`', '`'), ('[', ']')]
                .into_iter()
                .find_map(|(open, close)| {
                    let inner = ident.strip_prefix(open)?.strip_suffix(close)?;
                    Some(inner.replace(&format!("{close}{close}"), &close.to_string()))
                });
        unquoted.unwrap_or_else(|| ident.to_string())
    }
}

/// `(kind, name)` of a schema table or view that could not be parsed.
pub type SkippedObject = (&'static str, String);

/// Inference fails outright only for `SUM(column)` of a column missing from a schema table.
type Inferred<T> = Result<T, InferError>;

/// Why a query's types cannot be inferred at all, as opposed to being unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InferError {
    /// `SUM(column)` names a column that is not in the schema table it reads.
    SumColumnNotInSchema {
        /// The column as the query spells it.
        column: String,
        /// The schema table the column was looked up in.
        table: String,
    },
}

impl fmt::Display for InferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InferError::SumColumnNotInSchema { column, table } => write!(
                f,
                "Column '{column}' not found in table '{table}' in schema. \
                 If you want to query columns not in the schema, use an explicit cast: \
                 SUM({column}::TYPE)"
            ),
        }
    }
}

impl std::error::Error for InferError {}
