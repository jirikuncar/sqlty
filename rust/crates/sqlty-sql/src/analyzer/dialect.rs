//! The SQL dialects the analyzer reads, and the sqlparser dialect each maps to.

use sqlparser::dialect::{
    dialect_from_str, ClickHouseDialect, GenericDialect, MySqlDialect, PostgreSqlDialect,
    SQLiteDialect,
};

/// A SQL dialect: PostgreSQL, MySQL and SQLite are specified, the others follow sqlparser.
///
/// Parse a user's name with [`str::parse`], which rejects names it does not know
/// ([`UnknownDialect`]); [`SqlDialect::name`] is the canonical spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlDialect {
    /// PostgreSQL.
    Postgres,
    /// MySQL and MariaDB.
    MySql,
    /// SQLite.
    Sqlite,
    /// ClickHouse.
    ClickHouse,
    /// Another sqlparser dialect, by its name (`generic` when sqlparser has none).
    Other(&'static str),
}

/// sqlparser dialects beyond the special-cased ones; `tsql` is read as `mssql`.
pub(super) const OTHER_DIALECTS: [&str; 12] = [
    "ansi",
    "bigquery",
    "databricks",
    "duckdb",
    "hive",
    "mssql",
    "oracle",
    "redshift",
    "snowflake",
    "spark",
    "sparksql",
    "teradata",
];

/// A dialect name sqlty does not know.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("unknown dialect {name:?}; expected one of: {}", SqlDialect::NAMES.join(", "))]
pub struct UnknownDialect {
    /// The name as given.
    pub name: String,
}

impl std::str::FromStr for SqlDialect {
    type Err = UnknownDialect;

    /// A dialect from its name, case-insensitively (`postgresql` and `tsql` are aliases).
    fn from_str(name: &str) -> Result<Self, UnknownDialect> {
        let lower = name.to_ascii_lowercase();
        if Self::NAMES.contains(&lower.as_str()) || matches!(lower.as_str(), "postgresql" | "tsql")
        {
            Ok(Self::from_name(&lower))
        } else {
            Err(UnknownDialect {
                name: name.to_string(),
            })
        }
    }
}

impl SqlDialect {
    /// Every dialect name [`str::parse`] accepts, besides the aliases `postgresql` and `tsql`.
    pub const NAMES: [&'static str; 17] = [
        "postgres",
        "mysql",
        "sqlite",
        "clickhouse",
        "generic",
        "ansi",
        "bigquery",
        "databricks",
        "duckdb",
        "hive",
        "mssql",
        "oracle",
        "redshift",
        "snowflake",
        "spark",
        "sparksql",
        "teradata",
    ];

    /// The canonical name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::MySql => "mysql",
            Self::Sqlite => "sqlite",
            Self::ClickHouse => "clickhouse",
            Self::Other(name) => name,
        }
    }

    pub(crate) fn from_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" => Self::Postgres,
            "mysql" => Self::MySql,
            "sqlite" => Self::Sqlite,
            "clickhouse" => Self::ClickHouse,
            "tsql" => Self::Other("mssql"),
            other => Self::Other(
                OTHER_DIALECTS
                    .into_iter()
                    .find(|known| *known == other)
                    .unwrap_or("generic"),
            ),
        }
    }

    /// Whether the dialect reads PostgreSQL's `x::type` cast.
    pub(crate) fn has_cast_operator(self) -> bool {
        match self {
            Self::Postgres | Self::ClickHouse => true,
            Self::MySql | Self::Sqlite => false,
            Self::Other(name) => matches!(
                name,
                "databricks"
                    | "duckdb"
                    | "generic"
                    | "redshift"
                    | "snowflake"
                    | "spark"
                    | "sparksql"
            ),
        }
    }

    pub(crate) fn parser_dialect(self) -> Box<dyn sqlparser::dialect::Dialect> {
        match self {
            Self::Postgres => Box::new(PostgreSqlDialect {}),
            Self::MySql => Box::new(MySqlDialect {}),
            Self::Sqlite => Box::new(SQLiteDialect {}),
            Self::ClickHouse => Box::new(ClickHouseDialect {}),
            Self::Other(name) => dialect_from_str(name).unwrap_or_else(|| Box::new(GenericDialect)),
        }
    }
}
