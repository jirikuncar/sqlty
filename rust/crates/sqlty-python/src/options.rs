//! Settings every pass shares, parsed once at the boundary.

use sqlty_sql::analyzer::SchemaData;

use crate::rewriter::ImportStyle;
use crate::rules::RuleConfigError;

/// A setting the caller passed that sqlty does not support.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The `import-style` setting is not one of the supported spellings.
    #[error("Unsupported import-style: '{0}'")]
    ImportStyle(String),
    /// A rule or severity override is not known.
    #[error(transparent)]
    Rules(#[from] RuleConfigError),
}

/// The SQL dialect a query is parsed with.
///
/// `sqlty-sql` resolves the name (unknown names read as the generic dialect), so it is
/// kept as spelled; what this crate needs to know about it is parsed once here.
#[derive(Clone, Copy, Debug)]
pub struct Dialect<'a> {
    name: &'a str,
    mysql: bool,
}

impl<'a> Dialect<'a> {
    /// A dialect from the name the user configured (`postgres`, `mysql`, ...).
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            mysql: name.eq_ignore_ascii_case("mysql"),
        }
    }

    /// The name `sqlty-sql` takes.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// MySQL and MariaDB read `--sql` differently from a comment (see `mysql-sql-marker`).
    pub fn is_mysql(&self) -> bool {
        self.mysql
    }
}

/// What check, rewrite and fix are told besides the source.
pub struct Options<'a> {
    /// The dialect queries are parsed with.
    pub dialect: Dialect<'a>,
    /// The schema queries are read against, when there is one.
    pub schema: Option<&'a SchemaData>,
    /// How the `SQL` wrapper is imported and spelled.
    pub style: ImportStyle,
    /// Wrap row-returning queries of unknown row type as `SQL[UNTYPED]`.
    pub untyped: bool,
}

impl<'a> Options<'a> {
    /// Parse the settings Python passes: `import_style` is validated here, once.
    pub fn new(
        dialect: &'a str,
        schema: Option<&'a SchemaData>,
        import_style: Option<&str>,
        untyped: bool,
    ) -> Result<Self, ConfigError> {
        Ok(Self {
            dialect: Dialect::new(dialect),
            schema,
            style: ImportStyle::parse(import_style)?,
            untyped,
        })
    }
}
