//! The signatures of each dialect's built-in functions: parameters, result and the page of
//! the vendor documentation, compiled from `functions/<dialect>.txt` (see
//! `scripts/generate_function_catalog.py`).
//!
//! ```
//! use sqlty_sql::signatures::signatures;
//! use sqlty_sql::Dialect;
//!
//! let found = signatures(Dialect::Postgres, "LTRIM");
//! let text = found.iter().find(|s| s.label.contains("string: text")).unwrap();
//! assert_eq!(text.label, "ltrim(string: text[, characters: text]) -> text");
//! assert_eq!(text.parameter_label(1), Some("characters: text"));
//! assert!(text.accepts(1) && text.accepts(2) && !text.accepts(3));
//! assert_eq!(
//!     text.url(),
//!     "https://www.postgresql.org/docs/17/functions-string.html#FUNCTIONS-STRING-SQL"
//! );
//! ```

use crate::analyzer::SqlDialect;

/// One overload of a built-in function.
#[derive(Debug, PartialEq, Eq)]
pub struct Signature {
    /// The name, lower case.
    pub name: &'static str,
    /// The overload as the documentation writes it: `name(parameters) -> result`, or
    /// `name(parameters)`.
    pub label: &'static str,
    /// The result type, `setof ...` for a set-returning function; empty when the documentation
    /// gives none.
    pub returns: &'static str,
    /// The parameters, in order.
    pub parameters: &'static [Parameter],
    /// How many parameters a call must pass.
    pub required: usize,
    /// Whether the last parameter repeats.
    pub variadic: bool,
    docs: &'static str,
    page: &'static str,
}

/// One parameter of a [`Signature`].
#[derive(Debug, PartialEq, Eq)]
pub struct Parameter {
    /// Its name, when the documentation gives one.
    pub name: Option<&'static str>,
    /// The bytes of [`Signature::label`] that write it.
    pub label: (usize, usize),
    /// Whether a call may leave it out.
    pub optional: bool,
}

pub(crate) struct Catalog {
    /// Sorted by name.
    signatures: &'static [Signature],
}

include!(concat!(env!("OUT_DIR"), "/function_catalog.rs"));

impl Signature {
    /// Whether a call with `count` arguments fits this overload.
    pub fn accepts(&self, count: usize) -> bool {
        count >= self.required && (self.variadic || count <= self.parameters.len())
    }

    /// The parameter argument `index` (from 0) of a call binds to; `None` past the last
    /// parameter of an overload that does not repeat it.
    pub fn parameter(&self, index: usize) -> Option<usize> {
        match self.parameters.len() {
            0 => None,
            len if index < len => Some(index),
            len => self.variadic.then_some(len - 1),
        }
    }

    /// How [`Signature::label`] writes parameter `index`.
    pub fn parameter_label(&self, index: usize) -> Option<&'static str> {
        let (start, end) = self.parameters.get(index)?.label;
        self.label.get(start..end)
    }

    /// The page of the vendor documentation that describes the function.
    pub fn url(&self) -> String {
        format!("{}{}", self.docs, self.page)
    }
}

/// The catalogs `dialect` reads, in order: its own, or for `generic` those it extends.
fn catalogs(dialect: SqlDialect) -> &'static [&'static Catalog] {
    static PG: [&Catalog; 1] = [&POSTGRES];
    static DUCK: [&Catalog; 1] = [&DUCKDB];
    static MY: [&Catalog; 1] = [&MYSQL];
    match dialect {
        SqlDialect::Postgres | SqlDialect::Other("generic") => &PG,
        SqlDialect::Other("duckdb") => &DUCK,
        SqlDialect::MySql => &MY,
        _ => &[],
    }
}

/// The overloads of the built-in function `name` of `dialect`, case-insensitively; empty
/// for a function the catalog does not list. O(log n) in the size of the catalog.
pub fn signatures(dialect: SqlDialect, name: &str) -> &'static [Signature] {
    for catalog in catalogs(dialect) {
        let all = catalog.signatures;
        let start = all.partition_point(|s| cmp_folded(s.name, name).is_lt());
        let end = start + all[start..].partition_point(|s| cmp_folded(s.name, name).is_eq());
        if start < end {
            return &all[start..end];
        }
    }
    &[]
}

/// `lower` against `name` folded to lower case, without allocating.
fn cmp_folded(lower: &str, name: &str) -> std::cmp::Ordering {
    lower
        .bytes()
        .cmp(name.bytes().map(|b| b.to_ascii_lowercase()))
}
