//! The type of an expression: its Python type and its SQL type.

use std::rc::Rc;

use super::{python_type, SqlDialect};

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
}

impl Typed {
    /// Nothing is known.
    pub(super) const UNKNOWN: Typed = Typed {
        python: None,
        sql: None,
    };

    pub(super) fn new(python: Option<String>, sql: Option<String>) -> Self {
        Typed {
            python: python.map(Rc::from),
            sql: sql.map(Rc::from),
        }
    }

    /// A Python type with no SQL type, as a constant has.
    pub(super) fn python_only(python: &str) -> Self {
        Typed {
            python: Some(Rc::from(python)),
            sql: None,
        }
    }

    /// The type of a column of SQL type `sql`: the Python type is the one it maps to.
    pub(super) fn of_sql(sql: &str, dialect: SqlDialect) -> Self {
        Typed {
            python: python_type(sql, dialect).map(Rc::from),
            sql: Some(Rc::from(sql)),
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
