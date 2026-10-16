//! Settings every pass shares, parsed once at the boundary.

use sqlty_sql::analyzer::SchemaData;
pub use sqlty_sql::{Dialect, UnknownDialect};

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
    /// The dialect is not one sqlty knows.
    #[error(transparent)]
    Dialect(#[from] UnknownDialect),
}

/// What check, rewrite and fix are told besides the source.
pub struct Options<'a> {
    /// The dialect queries are parsed with.
    pub dialect: Dialect,
    /// The schema queries are read against, when there is one.
    pub schema: Option<&'a SchemaData>,
    /// How the `SQL` wrapper is imported and spelled.
    pub style: ImportStyle,
    /// Wrap row-returning queries of unknown row type as `SQL[UNTYPED]`.
    pub untyped: bool,
}

impl<'a> Options<'a> {
    /// Parse the settings Python passes: `dialect` and `import_style` are validated here, once.
    pub fn new(
        dialect: &str,
        schema: Option<&'a SchemaData>,
        import_style: Option<&str>,
        untyped: bool,
    ) -> Result<Self, ConfigError> {
        Ok(Self {
            dialect: dialect.parse()?,
            schema,
            style: ImportStyle::parse(import_style)?,
            untyped,
        })
    }
}
