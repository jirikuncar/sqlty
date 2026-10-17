//! SQL types as the analyzer reads them: built once from a sqlparser `DataType`, spelled
//! canonically for `sql_type` (`INTEGER` is `INT`, `INT[]` is `ARRAY<INT>`), and mapped to
//! the Python type the dialect's usual driver loads.
//!
//! A canonical spelling read back (a schema column's type, a rule's result) is parsed into
//! the same structure, so containers keep their element and field types either way.

use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use sqlparser::ast::{ArrayElemTypeDef, DataType, StructField};
use sqlparser::parser::Parser;

use super::dialect::SqlDialect;

pub(super) const ANY: &str = "typing.Any";
pub(super) const DECIMAL: &str = "decimal.Decimal";
/// What psycopg loads `inet` as: an address, or an interface when the value has a prefix.
const INET: &str = "ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface";
const CIDR: &str = "ipaddress.IPv4Network | ipaddress.IPv6Network";

/// A SQL type, with the structure of its containers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SqlType {
    /// A scalar type by its canonical spelling, parameters included: `DECIMAL(10, 2)`.
    Scalar(String),
    /// `T[]`, `T ARRAY`, `T[3]`; a bare `ARRAY` has no element type.
    Array {
        element: Option<Box<SqlType>>,
        size: Option<u64>,
    },
    /// `ROW(a INT, b TEXT)` and ClickHouse's `Tuple(...)`: fields in order.
    Row(Vec<(Option<String>, SqlType)>),
    /// DuckDB's and BigQuery's `STRUCT(a INT, b TEXT)`.
    Struct(Vec<(String, SqlType)>),
    /// `MAP(K, V)`.
    Map(Box<SqlType>, Box<SqlType>),
    /// DuckDB's tagged `UNION(a INT, b TEXT)`.
    Union(Vec<(String, SqlType)>),
    /// `INTERVAL`, with any qualifier (`DAY TO SECOND`) or precision dropped.
    Interval,
    /// A type sqlparser reads that has no Python type: a table type, a geometric type,
    /// MySQL's `SET`, ... Spelled as before.
    Unmapped(String),
}

/// Built-in PostgreSQL types sqlty has no Python type for: ranges and multiranges, which
/// psycopg loads as its own `Range`, and the geometric and snapshot types.
const UNMAPPED_BUILTINS: [&str; 21] = [
    "INT4RANGE",
    "INT8RANGE",
    "NUMRANGE",
    "TSRANGE",
    "TSTZRANGE",
    "DATERANGE",
    "INT4MULTIRANGE",
    "INT8MULTIRANGE",
    "NUMMULTIRANGE",
    "TSMULTIRANGE",
    "TSTZMULTIRANGE",
    "DATEMULTIRANGE",
    "POINT",
    "LINE",
    "LSEG",
    "BOX",
    "PATH",
    "POLYGON",
    "CIRCLE",
    "PG_LSN",
    "PG_SNAPSHOT",
];

/// Whether `dt` is a type the dialect itself defines that has no Python type: unlike a
/// name the schema may declare, it is unmapped with or without a schema.
pub(crate) fn is_unmapped_builtin(dt: &DataType, dialect: SqlDialect) -> bool {
    match SqlType::of(dt, dialect) {
        SqlType::Unmapped(_) => true,
        SqlType::Scalar(name) => {
            let base = name.split('(').next().unwrap_or_default();
            dialect == SqlDialect::Postgres && UNMAPPED_BUILTINS.contains(&base)
        }
        _ => false,
    }
}

/// Canonical spelling of a SQL type, e.g. `INTEGER` -> `INT`, `INT[]` -> `ARRAY<INT>`.
pub(crate) fn canonical_type(dt: &DataType, dialect: SqlDialect) -> String {
    SqlType::of(dt, dialect).to_string()
}

/// Map a canonical SQL type to a Python type string; unknown types are `None`.
/// `ARRAY<T>` is `list[T]` (bare `list` when `T` is unknown).
pub(super) fn python_type(sql_type: &str, dialect: SqlDialect) -> Option<String> {
    SqlType::parse(sql_type).python(dialect)
}

impl SqlType {
    /// The type `dt` names in `dialect`.
    #[deny(clippy::wildcard_enum_match_arm)]
    pub(crate) fn of(dt: &DataType, dialect: SqlDialect) -> SqlType {
        let of = |dt: &DataType| SqlType::of(dt, dialect);
        match dt {
            DataType::Array(element) => match element {
                ArrayElemTypeDef::None => SqlType::Array {
                    element: None,
                    size: None,
                },
                ArrayElemTypeDef::AngleBracket(inner) | ArrayElemTypeDef::Parenthesis(inner) => {
                    SqlType::Array {
                        element: Some(Box::new(of(inner))),
                        size: None,
                    }
                }
                ArrayElemTypeDef::SquareBracket(inner, size)
                | ArrayElemTypeDef::Qualified(inner, size) => SqlType::Array {
                    element: Some(Box::new(of(inner))),
                    size: *size,
                },
            },
            DataType::Struct(fields, _) => SqlType::Struct(
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| (field_name(field, i), of(&field.field_type)))
                    .collect(),
            ),
            DataType::Tuple(fields) => SqlType::Row(
                fields
                    .iter()
                    .map(|field| {
                        let name = field.field_name.as_ref().map(|name| name.value.clone());
                        (name, of(&field.field_type))
                    })
                    .collect(),
            ),
            DataType::Map(key, value, _) => SqlType::Map(Box::new(of(key)), Box::new(of(value))),
            DataType::Union(fields) => SqlType::Union(
                fields
                    .iter()
                    .map(|field| (field.field_name.value.clone(), of(&field.field_type)))
                    .collect(),
            ),
            DataType::Interval { .. } => SqlType::Interval,
            // ClickHouse wrappers that change the column, not its values.
            DataType::Nullable(inner) | DataType::LowCardinality(inner) => of(inner),
            DataType::Custom(name, args) => {
                let text = name.to_string().to_uppercase();
                custom(&text, args, dialect).unwrap_or_else(|| SqlType::Scalar(scalar(dt, dialect)))
            }
            DataType::Table(_)
            | DataType::NamedTable { .. }
            | DataType::Nested(_)
            | DataType::Unspecified
            | DataType::Trigger
            | DataType::AnyType
            | DataType::GeometricType(_) => SqlType::Unmapped(scalar(dt, dialect)),
            DataType::Character(_)
            | DataType::Char(_)
            | DataType::CharacterVarying(_)
            | DataType::CharVarying(_)
            | DataType::Varchar(_)
            | DataType::Nvarchar(_)
            | DataType::Uuid
            | DataType::CharacterLargeObject(_)
            | DataType::CharLargeObject(_)
            | DataType::Clob(_)
            | DataType::Binary(_)
            | DataType::Varbinary(_)
            | DataType::Blob(_)
            | DataType::TinyBlob
            | DataType::MediumBlob
            | DataType::LongBlob
            | DataType::Bytes(_)
            | DataType::Numeric(_)
            | DataType::Decimal(_)
            | DataType::DecimalUnsigned(_)
            | DataType::BigNumeric(_)
            | DataType::BigDecimal(_)
            | DataType::Dec(_)
            | DataType::DecUnsigned(_)
            | DataType::Float(_)
            | DataType::FloatUnsigned(_)
            | DataType::TinyInt(_)
            | DataType::TinyIntUnsigned(_)
            | DataType::UTinyInt
            | DataType::Int2(_)
            | DataType::Int2Unsigned(_)
            | DataType::SmallInt(_)
            | DataType::SmallIntUnsigned(_)
            | DataType::USmallInt
            | DataType::MediumInt(_)
            | DataType::MediumIntUnsigned(_)
            | DataType::Int(_)
            | DataType::Int4(_)
            | DataType::Int8(_)
            | DataType::Int16
            | DataType::Int32
            | DataType::Int64
            | DataType::Int128
            | DataType::Int256
            | DataType::Integer(_)
            | DataType::IntUnsigned(_)
            | DataType::Int4Unsigned(_)
            | DataType::IntegerUnsigned(_)
            | DataType::HugeInt
            | DataType::UHugeInt
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt32
            | DataType::UInt64
            | DataType::UInt128
            | DataType::UInt256
            | DataType::BigInt(_)
            | DataType::BigIntUnsigned(_)
            | DataType::UBigInt
            | DataType::Int8Unsigned(_)
            | DataType::Signed
            | DataType::SignedInteger
            | DataType::Unsigned
            | DataType::UnsignedInteger
            | DataType::Float4
            | DataType::Float32
            | DataType::Float64
            | DataType::Real
            | DataType::RealUnsigned
            | DataType::Float8
            | DataType::Double(_)
            | DataType::DoubleUnsigned(_)
            | DataType::DoublePrecision
            | DataType::DoublePrecisionUnsigned
            | DataType::Bool
            | DataType::Boolean
            | DataType::Date
            | DataType::Date32
            | DataType::Time(..)
            | DataType::Datetime(_)
            | DataType::Datetime64(..)
            | DataType::Timestamp(..)
            | DataType::TimestampNtz(_)
            | DataType::JSON
            | DataType::JSONB
            | DataType::Regclass
            | DataType::Text
            | DataType::TinyText
            | DataType::MediumText
            | DataType::LongText
            | DataType::String(_)
            | DataType::FixedString(_)
            | DataType::Bytea
            | DataType::Bit(_)
            | DataType::BitVarying(_)
            | DataType::VarBit(_)
            | DataType::Enum(..)
            | DataType::Set(_)
            | DataType::TsVector
            | DataType::TsQuery => SqlType::Scalar(scalar(dt, dialect)),
        }
    }

    /// A canonical spelling read back: the inverse of `Display`.
    pub(crate) fn parse(sql_type: &str) -> SqlType {
        if sql_type == "ARRAY" {
            return SqlType::Array {
                element: None,
                size: None,
            };
        }
        if let Some(inner) = sql_type.strip_prefix("ARRAY<") {
            let close = inner.rfind('>').unwrap_or(inner.len());
            let size = inner[close..]
                .strip_prefix(">[")
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|n| n.parse().ok());
            return SqlType::Array {
                element: Some(Box::new(SqlType::parse(&inner[..close]))),
                size,
            };
        }
        if sql_type == "INTERVAL" {
            return SqlType::Interval;
        }
        let fields = |prefix: &str| {
            let inner = sql_type.strip_prefix(prefix)?.strip_suffix(')')?;
            Some(
                split_top_level(inner)
                    .into_iter()
                    .map(|field| match field.split_once(' ') {
                        Some((name, ty)) => (Some(name.to_string()), SqlType::parse(ty.trim())),
                        None => (None, SqlType::parse(field)),
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let named = |fields: Vec<(Option<String>, SqlType)>| -> Vec<(String, SqlType)> {
            fields
                .into_iter()
                .enumerate()
                .map(|(i, (name, ty))| (name.unwrap_or_else(|| format!("f{}", i + 1)), ty))
                .collect()
        };
        if let Some(fields) = fields("ROW(") {
            return SqlType::Row(fields);
        }
        if let Some(fields) = fields("STRUCT(") {
            return SqlType::Struct(named(fields));
        }
        if let Some(fields) = fields("UNION(") {
            return SqlType::Union(named(fields));
        }
        if let Some(inner) = sql_type
            .strip_prefix("MAP(")
            .and_then(|s| s.strip_suffix(')'))
        {
            if let [key, value] = split_top_level(inner).as_slice() {
                return SqlType::Map(
                    Box::new(SqlType::parse(key)),
                    Box::new(SqlType::parse(value)),
                );
            }
        }
        SqlType::Scalar(sql_type.to_string())
    }

    /// The Python type the dialect's usual driver loads values of this type as.
    pub(crate) fn python(&self, dialect: SqlDialect) -> Option<String> {
        let any = |ty: &SqlType| ty.python(dialect).unwrap_or_else(|| ANY.to_string());
        match self {
            // DuckDB's fixed-size `ARRAY` comes back as a tuple, its `LIST` as a list.
            SqlType::Array {
                element,
                size: Some(_),
            } if dialect == SqlDialect::Other("duckdb") => Some(format!(
                "tuple[{}, ...]",
                element.as_deref().map_or_else(|| ANY.to_string(), any)
            )),
            SqlType::Array { element, .. } => {
                Some(match element.as_deref().and_then(|e| e.python(dialect)) {
                    Some(item) => format!("list[{item}]"),
                    None => "list".to_string(),
                })
            }
            SqlType::Row(fields) if fields.is_empty() => None,
            SqlType::Row(fields) => Some(format!(
                "tuple[{}]",
                fields
                    .iter()
                    .map(|(_, ty)| any(ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            SqlType::Struct(fields) => Some(format!(
                "dict[str, {}]",
                union(fields.iter().map(|(_, ty)| any(ty)))
            )),
            SqlType::Map(key, value) => {
                Some(format!("dict[{}, {}]", key.python(dialect)?, any(value)))
            }
            SqlType::Union(fields) => {
                let members: Option<Vec<String>> =
                    fields.iter().map(|(_, ty)| ty.python(dialect)).collect();
                Some(union(members?))
            }
            SqlType::Interval => Some("datetime.timedelta".to_string()),
            SqlType::Unmapped(_) => None,
            SqlType::Scalar(name) => scalar_python(name, dialect),
        }
    }
}

impl fmt::Display for SqlType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fields =
            |f: &mut fmt::Formatter<'_>,
             fields: &mut dyn Iterator<Item = (Option<&str>, &SqlType)>| {
                for (i, (name, ty)) in fields.enumerate() {
                    let comma = if i == 0 { "" } else { ", " };
                    match name {
                        Some(name) => write!(f, "{comma}{name} {ty}")?,
                        None => write!(f, "{comma}{ty}")?,
                    }
                }
                write!(f, ")")
            };
        match self {
            SqlType::Scalar(text) | SqlType::Unmapped(text) => write!(f, "{text}"),
            SqlType::Array { element: None, .. } => write!(f, "ARRAY"),
            SqlType::Array {
                element: Some(element),
                size,
            } => match size {
                Some(size) => write!(f, "ARRAY<{element}>[{size}]"),
                None => write!(f, "ARRAY<{element}>"),
            },
            SqlType::Row(row) => {
                write!(f, "ROW(")?;
                fields(f, &mut row.iter().map(|(name, ty)| (name.as_deref(), ty)))
            }
            SqlType::Struct(row) => {
                write!(f, "STRUCT(")?;
                fields(
                    f,
                    &mut row.iter().map(|(name, ty)| (Some(name.as_str()), ty)),
                )
            }
            SqlType::Union(row) => {
                write!(f, "UNION(")?;
                fields(
                    f,
                    &mut row.iter().map(|(name, ty)| (Some(name.as_str()), ty)),
                )
            }
            SqlType::Map(key, value) => write!(f, "MAP({key}, {value})"),
            SqlType::Interval => write!(f, "INTERVAL"),
        }
    }
}

/// A container written as a custom type, as sqlparser reads `ROW(...)`, `STRUCT(...)`,
/// `MAP(...)` and `UNION(...)` outside the dialects it knows them in: its arguments come as
/// words, `name TYPE` pairs alternating.
fn custom(name: &str, args: &[String], dialect: SqlDialect) -> Option<SqlType> {
    let ty = |text: &str| -> Option<SqlType> {
        let parsed = Parser::new(dialect.parser_dialect().as_ref())
            .try_with_sql(text)
            .ok()?
            .parse_data_type()
            .ok()?;
        Some(SqlType::of(&parsed, dialect))
    };
    let pairs = || -> Option<Vec<(String, SqlType)>> {
        (!args.is_empty() && args.len().is_multiple_of(2)).then(|| {
            args.chunks(2)
                .map(|pair| Some((pair[0].clone(), ty(&pair[1])?)))
                .collect()
        })?
    };
    match name {
        "ROW" => Some(SqlType::Row(
            pairs()?
                .into_iter()
                .map(|(name, ty)| (Some(name), ty))
                .collect(),
        )),
        "STRUCT" => Some(SqlType::Struct(pairs()?)),
        "UNION" => Some(SqlType::Union(pairs()?)),
        "MAP" => match args {
            [key, value] => Some(SqlType::Map(Box::new(ty(key)?), Box::new(ty(value)?))),
            _ => None,
        },
        _ => None,
    }
}

fn field_name(field: &StructField, index: usize) -> String {
    field
        .field_name
        .as_ref()
        .map_or_else(|| format!("f{}", index + 1), |name| name.value.clone())
}

/// `parts` joined as a union, each member once, in order.
fn union(parts: impl IntoIterator<Item = String>) -> String {
    let mut members: Vec<String> = Vec::new();
    for part in parts {
        for member in part.split(" | ") {
            if !members.iter().any(|m| m == member) {
                members.push(member.to_string());
            }
        }
    }
    members.join(" | ")
}

/// `text` split at the commas outside parentheses and angle brackets.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in text.char_indices() {
        match c {
            '(' | '<' => depth += 1,
            ')' | '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts
}

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

/// The canonical spelling of a scalar type: its name normalized across dialects
/// (`INTEGER` -> `INT`, `TIMESTAMP WITH TIME ZONE` -> `TIMESTAMPTZ`), parameters kept.
fn scalar(dt: &DataType, dialect: SqlDialect) -> String {
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
        "CHARACTER LARGE OBJECT" | "CHAR LARGE OBJECT" => "TEXT",
        "BINARY LARGE OBJECT" => "VARBINARY",
        "BIT VARYING" => "VARBIT",
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

/// Every Python type a SQL type maps to, one union member at a time (`int`,
/// `ipaddress.IPv4Address`, ...): what the row-type checks must know.
pub fn python_types() -> std::collections::BTreeSet<&'static str> {
    SQL_TO_PYTHON_TYPE
        .values()
        .flat_map(|python| python.split(" | "))
        .chain([DECIMAL, "float", "datetime.timedelta", "bytes", "str", ANY])
        .collect()
}

/// What the dialect's usual drivers load exact decimals as: SQLite and Oracle drivers
/// return floats, the others `decimal.Decimal`.
pub(super) fn decimal_type(dialect: SqlDialect) -> &'static str {
    match dialect {
        SqlDialect::Sqlite | SqlDialect::Other("oracle") => "float",
        _ => DECIMAL,
    }
}

/// The Python type of a scalar type by its canonical spelling; unknown types are `None`.
fn scalar_python(sql_type: &str, dialect: SqlDialect) -> Option<String> {
    let base = sql_type.split(['(', '<']).next().unwrap_or_default().trim();
    match base {
        "DECIMAL" | "UDECIMAL" | "BIGDECIMAL" | "DECFLOAT" => {
            Some(decimal_type(dialect).to_string())
        }
        // SQLite has no date or boolean storage class: sqlite3 returns the stored text and
        // integers (https://www.sqlite.org/datatype3.html, 2.1 and 2.2).
        "DATE" | "TIME" | "TIMETZ" | "TIMESTAMP" | "TIMESTAMPTZ" | "DATETIME"
            if dialect == SqlDialect::Sqlite =>
        {
            Some("str".to_string())
        }
        "BOOLEAN" | "BOOL" if dialect == SqlDialect::Sqlite => Some("int".to_string()),
        // A MySQL `TIME` is a duration, up to 838 hours either way.
        "TIME" if dialect == SqlDialect::MySql => Some("datetime.timedelta".to_string()),
        // MySQL drivers load `BIT(n)` as bytes, psycopg loads bit strings as text.
        "BIT" | "VARBIT" if dialect == SqlDialect::MySql => Some("bytes".to_string()),
        // PyMySQL returns a `SET` as its comma-separated text, and spatial values in
        // MySQL's internal binary format. (`JSON` stays `Any`: text here, parsed by psycopg.)
        "SET" if dialect == SqlDialect::MySql => Some("str".to_string()),
        "GEOMETRY" | "POINT" | "LINESTRING" | "POLYGON" | "MULTIPOINT" | "MULTILINESTRING"
        | "MULTIPOLYGON" | "GEOMETRYCOLLECTION"
            if dialect == SqlDialect::MySql =>
        {
            Some("bytes".to_string())
        }
        // sqlite3 returns the stored text: SQLite has no UUID storage class.
        "UUID" if dialect == SqlDialect::Sqlite => Some("str".to_string()),
        "BIT" | "VARBIT" => Some("str".to_string()),
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
