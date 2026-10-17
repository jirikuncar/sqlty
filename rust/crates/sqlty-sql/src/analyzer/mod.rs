//! SQL type inference with sqlparser-rs.
//!
//! Rows come from the `SELECT`s of a query, typed across the branches of a set
//! operation, or from the top-level `RETURNING` of INSERT/UPDATE/DELETE/MERGE. Type
//! names are canonicalized (`INTEGER` becomes `INT`, `NUMERIC(10,2)` becomes
//! `DECIMAL(10, 2)`), as `SELECT *` expansion writes them back into queries.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use sqlparser::ast::{ArrayElemTypeDef, DataType};

mod api;
mod dialect;
mod dialect_functions;
mod dml;
mod expr;
mod functions;
mod hashing;
mod inference;
mod json;
mod literals;
mod memo;
mod names;
mod narrowing;
mod nullability;
mod operators;
mod params;
mod preprocess;
mod query;
mod references;
mod registry;
mod resolve;
mod scope;
mod star_expand;
mod table_functions;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod typed;

#[cfg(test)]
pub(crate) use api::load_schema;
pub use api::{
    analyze_select, analyze_with_diagnostics, composite_columns, infer_type,
    load_schema_with_skipped, parse_error, returns_rows,
};
pub(crate) use api::{typed, view_columns};
pub use dialect::{SqlDialect, UnknownDialect};
use expr::from_sql_type;
pub use nullability::Nullability;
pub use params::{analyze_params, Param};
pub(crate) use preprocess::parse_ddl;
pub use query::Column;
pub use star_expand::{expand_star, result_stars};
use typed::Typed;

const ANY: &str = "typing.Any";
const DECIMAL: &str = "decimal.Decimal";
/// What psycopg loads `inet` as: an address, or an interface when the value has a prefix.
const INET: &str = "ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface";
const CIDR: &str = "ipaddress.IPv4Network | ipaddress.IPv6Network";

/// Mapping from canonical SQL type names to Python type strings; `DECIMAL` types
/// are mapped by [`decimal_type`] and arrays by [`python_type`].
static SQL_TO_PYTHON_TYPE: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("INT", "int"),
        ("INTEGER", "int"),
        ("BIGINT", "int"),
        ("SMALLINT", "int"),
        ("TINYINT", "int"),
        ("MEDIUMINT", "int"),
        ("YEAR", "int"),
        ("SIGNED", "int"),
        ("UNSIGNED", "int"),
        ("UTINYINT", "int"),
        ("USMALLINT", "int"),
        ("UMEDIUMINT", "int"),
        ("UINT", "int"),
        ("UBIGINT", "int"),
        ("INT128", "int"),
        ("UINT128", "int"),
        ("INT256", "int"),
        ("UINT256", "int"),
        ("FLOAT", "float"),
        ("DOUBLE", "float"),
        ("REAL", "float"),
        ("UDOUBLE", "float"),
        ("TEXT", "str"),
        ("VARCHAR", "str"),
        ("CHAR", "str"),
        ("STRING", "str"),
        ("NAME", "str"),
        ("ENUM", "str"),
        ("CITEXT", "str"),
        ("BPCHAR", "str"),
        ("XML", "str"),
        ("MONEY", "str"),
        ("TSVECTOR", "str"),
        ("TSQUERY", "str"),
        ("MACADDR", "str"),
        ("MACADDR8", "str"),
        ("PG_NODE_TREE", "str"),
        ("ACLITEM", "str"),
        ("\"CHAR\"", "str"),
        ("OID", "int"),
        ("XID", "str"),
        ("XID8", "str"),
        ("CID", "str"),
        ("REGPROC", "str"),
        ("REGPROCEDURE", "str"),
        ("REGOPER", "str"),
        ("REGOPERATOR", "str"),
        ("REGCLASS", "str"),
        ("REGTYPE", "str"),
        ("REGCONFIG", "str"),
        ("REGDICTIONARY", "str"),
        ("REGNAMESPACE", "str"),
        ("REGROLE", "str"),
        ("REGCOLLATION", "str"),
        ("TID", "str"),
        ("ANYARRAY", "str"),
        ("INT2VECTOR", "str"),
        ("OIDVECTOR", "str"),
        ("SERIAL", "int"),
        ("SMALLSERIAL", "int"),
        ("BIGSERIAL", "int"),
        ("INET", INET),
        ("CIDR", CIDR),
        ("BOOLEAN", "bool"),
        ("BOOL", "bool"),
        ("DATE", "datetime.date"),
        ("TIMESTAMP", "datetime.datetime"),
        ("TIMESTAMPTZ", "datetime.datetime"),
        ("DATETIME", "datetime.datetime"),
        ("TIME", "datetime.time"),
        ("TIMETZ", "datetime.time"),
        ("INTERVAL", "datetime.timedelta"),
        ("JSON", ANY),
        ("JSONB", ANY),
        // psycopg loads an anonymous record as a tuple (of strings, in text format).
        ("RECORD", "tuple[typing.Any, ...]"),
        ("UUID", "uuid.UUID"),
        ("BYTEA", "bytes"),
        ("BLOB", "bytes"),
        ("BINARY", "bytes"),
        ("VARBINARY", "bytes"),
    ])
});

pub use crate::catalog::{Catalog as SchemaData, SchemaColumn};

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

/// Canonical spelling of a SQL type, e.g. `INTEGER` -> `INT`, `INT[]` -> `ARRAY<INT>`.
pub(crate) fn canonical_type(dt: &DataType, dialect: SqlDialect) -> String {
    // The common column types, spelled the same in every dialect, skip the
    // format-and-reparse below.
    let common = match dt {
        DataType::Integer(None) | DataType::Int(None) | DataType::Int4(None) => Some("INT"),
        DataType::BigInt(None) => Some("BIGINT"),
        DataType::SmallInt(None) => Some("SMALLINT"),
        DataType::Text => Some("TEXT"),
        DataType::Boolean | DataType::Bool => Some("BOOLEAN"),
        DataType::Uuid => Some("UUID"),
        DataType::Date => Some("DATE"),
        DataType::Bytea => Some("VARBINARY"),
        DataType::JSON => Some("JSON"),
        DataType::JSONB => Some("JSONB"),
        DataType::Real => Some("FLOAT"),
        DataType::DoublePrecision => Some("DOUBLE"),
        DataType::Varchar(None) => Some("VARCHAR"),
        DataType::Char(None) => Some("CHAR"),
        _ => None,
    };
    if let Some(common) = common {
        return common.to_string();
    }
    if let DataType::Array(elem) = dt {
        return match elem {
            ArrayElemTypeDef::None => "ARRAY".to_string(),
            ArrayElemTypeDef::AngleBracket(inner)
            | ArrayElemTypeDef::Parenthesis(inner)
            | ArrayElemTypeDef::Qualified(inner, None)
            | ArrayElemTypeDef::SquareBracket(inner, None) => {
                format!("ARRAY<{}>", canonical_type(inner, dialect))
            }
            ArrayElemTypeDef::SquareBracket(inner, Some(size))
            | ArrayElemTypeDef::Qualified(inner, Some(size)) => {
                format!("ARRAY<{}>[{size}]", canonical_type(inner, dialect))
            }
        };
    }
    let text = dt.to_string().to_uppercase();
    let (name, args) = match (text.find('('), text.find(')')) {
        (Some(open), Some(close)) if open < close => {
            let args: Vec<&str> = text[open + 1..close].split(',').map(str::trim).collect();
            (
                format!("{} {}", &text[..open], &text[close + 1..]),
                format!("({})", args.join(", ")),
            )
        }
        _ => (text.clone(), String::new()),
    };
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let (name, unsigned) = match name.strip_suffix(" UNSIGNED") {
        Some(name) => (name, true),
        None => (name.as_str(), false),
    };
    let mysql = dialect == SqlDialect::MySql;
    let base = match name {
        "TIMESTAMP WITH TIME ZONE" => "TIMESTAMPTZ",
        "TIME WITH TIME ZONE" => "TIMETZ",
        "TIMESTAMP WITHOUT TIME ZONE" => "TIMESTAMP",
        "TIME WITHOUT TIME ZONE" => "TIME",
        "INTEGER" | "INT4" => "INT",
        "INT1" => "TINYINT",
        "INT2" | "INT16" => "SMALLINT",
        "INT32" => "INT",
        "INT8" if dialect == SqlDialect::ClickHouse => "TINYINT",
        "INT8" | "INT64" => "BIGINT",
        "UINT8" => "UTINYINT",
        "UINT16" => "USMALLINT",
        "UINT32" => "UINT",
        "UINT64" => "UBIGINT",
        "HUGEINT" => "INT128",
        "UHUGEINT" => "UINT128",
        "FLOAT32" => "FLOAT",
        "FLOAT64" => "DOUBLE",
        "NUMBER" => "DECIMAL",
        "BIGNUMERIC" => "BIGDECIMAL",
        "VARCHAR2" | "NVARCHAR2" => "VARCHAR",
        "CLOB" => "TEXT",
        "BYTES" => "BINARY",
        "UNIQUEIDENTIFIER" => "UUID",
        "TIMESTAMP_NTZ" | "DATETIME2" => "TIMESTAMP",
        "TIMESTAMP_TZ" | "TIMESTAMP_LTZ" | "DATETIMEOFFSET" => "TIMESTAMPTZ",
        "FLOAT" if dialect == SqlDialect::Postgres => "DOUBLE",
        "FLOAT8" | "DOUBLE PRECISION" => "DOUBLE",
        "FLOAT4" | "REAL" => "FLOAT",
        "NUMERIC" | "DEC" | "FIXED" => "DECIMAL",
        "CHARACTER VARYING" | "NVARCHAR" => "VARCHAR",
        "CHARACTER" | "NCHAR" => "CHAR",
        "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" => "TEXT",
        "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" => "BLOB",
        "STRING" => "TEXT",
        "BOOL" => "BOOLEAN",
        "BYTEA" | "BLOB" => "VARBINARY",
        other => other,
    };
    let base = match (mysql, base) {
        (true, "TIMESTAMP") => "TIMESTAMPTZ",
        (true, "INT3" | "MIDDLEINT") => "MEDIUMINT",
        (true, "SIGNED" | "SIGNED INTEGER") => "BIGINT",
        (true, "UNSIGNED" | "UNSIGNED INTEGER") => "UBIGINT",
        _ if unsigned && base == "FLOAT" => "DOUBLE",
        _ => base,
    };
    let sign = if unsigned { "U" } else { "" };
    format!("{sign}{base}{args}")
}

/// What the dialect's usual drivers load exact decimals as: SQLite and Oracle drivers
/// return floats, the others `decimal.Decimal`.
/// Every Python type a SQL type maps to, one union member at a time (`int`,
/// `ipaddress.IPv4Address`, ...): what the row-type checks must know.
pub fn python_types() -> std::collections::BTreeSet<&'static str> {
    SQL_TO_PYTHON_TYPE
        .values()
        .flat_map(|python| python.split(" | "))
        .chain([DECIMAL, "float"])
        .collect()
}

fn decimal_type(dialect: SqlDialect) -> &'static str {
    match dialect {
        SqlDialect::Sqlite | SqlDialect::Other("oracle") => "float",
        _ => DECIMAL,
    }
}

/// Map a canonical SQL type to a Python type string; unknown types are `None`.
/// `ARRAY<T>` is `list[T]` (bare `list` when `T` is unknown).
fn python_type(sql_type: &str, dialect: SqlDialect) -> Option<String> {
    if let Some(inner) = sql_type.strip_prefix("ARRAY<") {
        let inner = &inner[..inner.rfind('>')?];
        return Some(match python_type(inner, dialect) {
            Some(item) => format!("list[{item}]"),
            None => "list".to_string(),
        });
    }
    let base = sql_type.split(['(', '<']).next().unwrap_or_default().trim();
    match base {
        "ARRAY" => Some("list".to_string()),
        "DECIMAL" | "UDECIMAL" | "BIGDECIMAL" => Some(decimal_type(dialect).to_string()),
        // A MySQL `TIME` is a duration, up to 838 hours either way.
        "TIME" if dialect == SqlDialect::MySql => Some("datetime.timedelta".to_string()),
        _ => SQL_TO_PYTHON_TYPE
            .get(base)
            .map(ToString::to_string)
            .or_else(|| (dialect == SqlDialect::Sqlite).then(|| sqlite_affinity(base))?),
    }
}

/// The Python type of SQLite's type affinity for a type name it has no mapping for, by the
/// rules of <https://www.sqlite.org/datatype3.html#determination_of_column_affinity>; no
/// type and `ANY` can hold anything.
fn sqlite_affinity(name: &str) -> Option<String> {
    let name = name.to_ascii_uppercase();
    let has = |part: &str| name.contains(part);
    Some(
        // `ANY` holds any value in a STRICT table.
        if name.is_empty() || name == "ANY" {
            return None;
        } else if has("INT") {
            "int"
        } else if has("CHAR") || has("CLOB") || has("TEXT") {
            "str"
        } else if has("BLOB") {
            "bytes"
        } else if has("REAL") || has("FLOA") || has("DOUB") {
            "float"
        } else {
            decimal_type(SqlDialect::Sqlite)
        }
        .to_string(),
    )
}
