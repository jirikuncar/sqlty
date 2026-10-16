//! The type of an expression: its Python type and its SQL type.

use std::rc::Rc;
use std::sync::Arc;

use super::{python_type, SqlDialect};
use crate::values::{self, Value};

/// What is known about the type of an expression. Either side can be unknown: a constant
/// has a Python type but no SQL type, an enum or a type the analyzer does not map has a SQL
/// type but no Python type.
///
/// The strings are shared, so passing a type around (`CASE`, `COALESCE`, a column read
/// several times) costs a reference count, not a copy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Typed {
    /// The Python type, e.g. `int` or `typing.Literal['a', 'b']`.
    pub(super) python: Option<Rc<str>>,
    /// The SQL type as the analyzer canonicalises it, e.g. `BIGINT`.
    pub(super) sql: Option<Rc<str>>,
    /// The values a `Literal` Python type lists, kept as data so sets merge and narrow
    /// without reading the rendered type back.
    pub(super) values: Option<Arc<[Value]>>,
}

impl Typed {
    /// Nothing is known.
    pub(super) const UNKNOWN: Typed = Typed {
        python: None,
        sql: None,
        values: None,
    };

    /// A type with no value set.
    pub(super) fn new(python: Option<String>, sql: Option<String>) -> Self {
        Typed {
            python: python.map(Rc::from),
            sql: sql.map(Rc::from),
            values: None,
        }
    }

    /// A type whose Python type is `python` and whose values are `values` when it is a
    /// `Literal` (the catalog and result columns carry both).
    pub(super) fn with_values(
        python: Option<String>,
        sql: Option<String>,
        values: Option<Arc<[Value]>>,
    ) -> Self {
        Typed {
            values,
            ..Typed::new(python, sql)
        }
    }

    /// The `Literal` of `set` (rendered once, here), with SQL type `sql`; `None` when the
    /// set cannot be a `Literal` (empty, mixed).
    pub(super) fn of_values(set: Vec<Value>, sql: Option<Rc<str>>) -> Option<Self> {
        let python = values::render(&set)?;
        Some(Typed {
            python: Some(Rc::from(python)),
            sql,
            values: Some(set.into()),
        })
    }

    /// A Python type with no SQL type, as a constant has.
    pub(super) fn python_only(python: &str) -> Self {
        Typed {
            python: Some(Rc::from(python)),
            sql: None,
            values: None,
        }
    }

    /// The type of a column of SQL type `sql`: the Python type is the one it maps to.
    pub(super) fn of_sql(sql: &str, dialect: SqlDialect) -> Self {
        Typed {
            python: python_type(sql, dialect).map(Rc::from),
            sql: Some(Rc::from(sql)),
            values: None,
        }
    }

    pub(super) fn python(&self) -> Option<&str> {
        self.python.as_deref()
    }

    pub(super) fn sql(&self) -> Option<&str> {
        self.sql.as_deref()
    }

    /// Whether a Python type is known.
    pub(super) fn is_known(&self) -> bool {
        self.python.is_some()
    }

    /// The `(python, sql)` pair as owned strings, for the public `Column`.
    pub(super) fn into_strings(self) -> (Option<String>, Option<String>) {
        (
            self.python.map(|ty| ty.to_string()),
            self.sql.map(|ty| ty.to_string()),
        )
    }
}

impl From<(Option<String>, Option<String>)> for Typed {
    fn from((python, sql): (Option<String>, Option<String>)) -> Self {
        Typed::new(python, sql)
    }
}
