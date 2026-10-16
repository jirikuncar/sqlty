//! The SQL dialects the analyzer reads, and the sqlparser dialect each maps to.

use sqlparser::dialect::{
    dialect_from_str, ClickHouseDialect, GenericDialect, MySqlDialect, PostgreSqlDialect,
    SQLiteDialect,
};

/// A dialect name, matched case-insensitively; names sqlparser doesn't know read as generic SQL.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SqlDialect {
    Postgres,
    MySql,
    Sqlite,
    ClickHouse,
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

impl SqlDialect {
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
