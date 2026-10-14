//! SQL type inference with sqlparser-rs.
//!
//! Rows come from the `SELECT`s of a query, typed across the branches of a set
//! operation, or from the top-level `RETURNING` of INSERT/UPDATE/DELETE/MERGE. Type
//! names are canonicalized (`INTEGER` becomes `INT`, `NUMERIC(10,2)` becomes
//! `DECIMAL(10, 2)`), as `SELECT *` expansion writes them back into queries.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;

use crate::diagnostics::{Diagnostics, NameDiagnostic, NameRule};
use regex::{Captures, Regex};
use sqlparser::ast::{
    visit_expressions, AccessExpr, ArrayElemTypeDef, BinaryOperator, DataType, Expr, FromTable,
    FunctionArg, FunctionArgExpr, FunctionArguments, Ident, Join, JoinConstraint, JoinOperator,
    ObjectName, OutputClause, Query, Select, SelectItem, SelectItemQualifiedWildcardKind, SetExpr,
    Statement, Subscript, TableAlias, TableFactor, TableObject, TableWithJoins, UnaryOperator,
    Value,
};
use sqlparser::dialect::{
    dialect_from_str, ClickHouseDialect, GenericDialect, MySqlDialect, PostgreSqlDialect,
    SQLiteDialect,
};
use sqlparser::parser::Parser;
use std::sync::LazyLock;

mod dml;
mod expr;
mod functions;
mod names;
mod narrowing;
mod nullability;
mod operators;
mod query;
mod resolve;
mod scope;

use dml::*;
use expr::*;
use functions::*;
use names::*;
use nullability::*;
use operators::*;
use query::*;

pub use nullability::Nullability;
pub use query::Column;
use scope::*;

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
        ("UUID", "uuid.UUID"),
        ("BYTEA", "bytes"),
        ("BLOB", "bytes"),
        ("BINARY", "bytes"),
        ("VARBINARY", "bytes"),
    ])
});

static STAR_ITEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(\w+)\s*\.\s*)?\*$").unwrap());

use crate::catalog::{fold, FunctionReturn};
pub use crate::catalog::{Catalog as SchemaData, SchemaColumn};

pub type Schema = Arc<SchemaData>;

impl SchemaColumn {
    fn typed(&self, dialect: SqlDialect) -> Typed {
        match &self.python_type {
            Some(python_type) => (Some(python_type.clone()), Some(self.sql_type.clone())),
            None => from_sql_type(self.sql_type.clone(), dialect),
        }
    }
}

/// `(kind, name)` of a schema table or view that could not be parsed.
pub type SkippedObject = (&'static str, String);

/// Inference fails outright only for `SUM(column)` of a column missing from a schema table.
type Inferred<T> = Result<T, String>;

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
const OTHER_DIALECTS: [&str; 12] = [
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

/// A leading `--sql` marker line: MySQL only reads `--` followed by whitespace as a comment.
static SQL_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\s*--sql[^\n]*\n?").unwrap());

/// PostgreSQL row-lock clauses sqlparser cannot read, respelled as the closest ones it can:
/// `NO KEY UPDATE` as `UPDATE`, `KEY SHARE` as `SHARE`, and `OF` keeps only its first table.
static POSTGRES_LOCK_FIXES: LazyLock<[(Regex, &str); 3]> = LazyLock::new(|| {
    const NAME: &str = r#"(?:"[^"]*"|\w+)(?:\s*\.\s*(?:"[^"]*"|\w+))*"#;
    [
        (
            Regex::new(r"(?i)\bFOR\s+NO\s+KEY\s+UPDATE\b").unwrap(),
            "FOR UPDATE",
        ),
        (
            Regex::new(r"(?i)\bFOR\s+KEY\s+SHARE\b").unwrap(),
            "FOR SHARE",
        ),
        (
            Regex::new(&format!(
                r"(?i)\b(FOR\s+(?:UPDATE|SHARE))\s+OF\s+({NAME})(?:\s*,\s*{NAME})*"
            ))
            .unwrap(),
            "$1 OF $2",
        ),
    ]
});

/// psycopg placeholders (`%s`, `%b`, `%t`, `%(name)s`, ...) and the `%%` escape.
static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"%%|%(?:\([^)]+\))?[sbt]").unwrap());

/// A `?` placeholder in group 1, or a literal, quoted name, comment or `?|`/`?&` operator to skip.
static QMARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"'(?:[^']|'')*'|"(?:[^"]|"")*"|--[^\n]*|(?s:/\*.*?\*/)|\?[|&]|(\?)"#).unwrap()
});

/// MySQL column types sqlparser rejects: `ZEROFILL` (which implies `UNSIGNED`), then
/// `INT3`/`MIDDLEINT` before `UNSIGNED`.
static MYSQL_TYPE_FIXES: LazyLock<[(Regex, &str); 2]> = LazyLock::new(|| {
    [
        (
            Regex::new(r"(?i)(?:\s+(?:UN)?SIGNED)?\s+ZEROFILL\b(?:\s+(?:UN)?SIGNED\b)?").unwrap(),
            " UNSIGNED",
        ),
        (
            Regex::new(r"(?i)\b(?:INT3|MIDDLEINT)\b(\s*\(\s*\d+\s*\))?(\s+UNSIGNED\b)").unwrap(),
            "MEDIUMINT$1$2",
        ),
    ]
});

/// Parse `sql`; `None` when sqlparser fails or [`reject_unsupported`] rejects it.
fn parse(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    let stmts = Parser::parse_sql(dialect.parser_dialect().as_ref(), sql).ok()?;
    (!reject_unsupported(&stmts, dialect)).then_some(stmts)
}

/// Parse schema DDL, with the MySQL column types sqlparser rejects respelled.
pub(crate) fn parse_ddl(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    if dialect != SqlDialect::MySql {
        return parse(sql, dialect);
    }
    let sql = MYSQL_TYPE_FIXES
        .iter()
        .fold(sql.to_string(), |sql, (pattern, replacement)| {
            pattern.replace_all(&sql, *replacement).into_owned()
        });
    parse(&sql, dialect)
}

/// Parse a query (not schema DDL) through [`parser_input`]. In PostgreSQL, where
/// `?` is also a JSONB operator, `?` placeholders become `$1`, `$2`, ... only when
/// the query doesn't parse as written.
fn parse_query(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    let sql = parser_input(sql, dialect);
    parse(&sql, dialect).or_else(|| {
        (dialect == SqlDialect::Postgres && sql.contains('?'))
            .then(|| parse(&numbered_placeholders(&sql), dialect))
            .flatten()
    })
}

/// `sql` as sqlparser should read it, in every dialect: the `--sql` marker line is
/// dropped (a leading space form like `--sql SELECT 1` leaves nothing), placeholders
/// become `NULL`, a value of unknown type, and `%%` becomes `%`. Like psycopg,
/// literals and comments are not skipped; any query this mangles fails to parse
/// and stays uninferred rather than mistyped.
fn parser_input(sql: &str, dialect: SqlDialect) -> String {
    let sql = SQL_MARKER.replace(sql, "");
    let sql = if dialect == SqlDialect::Postgres {
        let sql = drop_conflict_predicate(&sql);
        POSTGRES_LOCK_FIXES
            .iter()
            .fold(sql, |sql, (pattern, replacement)| {
                pattern.replace_all(&sql, *replacement).into_owned()
            })
    } else {
        sql.into_owned()
    };
    PLACEHOLDER
        .replace_all(&sql, |caps: &Captures| match &caps[0] {
            "%%" => "%",
            _ => "NULL",
        })
        .into_owned()
}

/// Drops the `WHERE <predicate>` of `ON CONFLICT (cols) WHERE <predicate> DO ...`, the
/// partial unique index that arbitrates the conflict; it does not change the result columns.
fn drop_conflict_predicate(sql: &str) -> String {
    static CONFLICT_TARGET: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\bON\s+CONFLICT\s*\(").unwrap());
    static WHERE_CLAUSE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\A\s*WHERE\b").unwrap());
    let mut out = String::with_capacity(sql.len());
    let mut last = 0;
    for target in CONFLICT_TARGET.find_iter(sql) {
        if target.start() < last {
            continue;
        }
        let Some(close) = closing_paren(sql, target.end()) else {
            break;
        };
        let Some(clause) = WHERE_CLAUSE.find(&sql[close + 1..]) else {
            continue;
        };
        let Some(end) = do_keyword(sql, close + 1 + clause.end()) else {
            continue;
        };
        out.push_str(&sql[last..=close]);
        out.push(' ');
        last = end;
    }
    out.push_str(&sql[last..]);
    out
}

/// Byte index of the `)` closing the parenthesis that ends just before `start`.
fn closing_paren(sql: &str, start: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    let (mut depth, mut i) = (1, start);
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => i = quoted_end(bytes, i),
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// Byte index of the first `DO` word at or after `start` outside parentheses and quotes.
fn do_keyword(sql: &str, start: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    let (mut depth, mut i) = (0, start);
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => {
                i = quoted_end(bytes, i);
                continue;
            }
            b'(' => depth += 1,
            b')' if depth == 0 => return None,
            b')' => depth -= 1,
            _ if depth == 0
                && bytes
                    .get(i..i + 2)
                    .is_some_and(|w| w.eq_ignore_ascii_case(b"DO"))
                && (i == 0 || !is_word(bytes[i - 1]))
                && bytes.get(i + 2).is_none_or(|&b| !is_word(b)) =>
            {
                return Some(i)
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Index just past the quoted string or identifier that opens at `start`; a doubled quote
/// stays inside it.
fn quoted_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            if bytes.get(i + 1) != Some(&quote) {
                return i + 1;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn numbered_placeholders(sql: &str) -> String {
    let mut n = 0;
    QMARK
        .replace_all(sql, |caps: &Captures| match caps.get(1) {
            Some(_) => {
                n += 1;
                format!("${n}")
            }
            None => caps[0].to_string(),
        })
        .into_owned()
}

/// Whether `stmts` use what sqlparser accepts but the dialect does not:
/// `CAST(x AS <not a type>)`, which sqlparser re-reads as a call to a function
/// named `CAST`, and MySQL-only types such as `SET('a')` in other dialects.
fn reject_unsupported(stmts: &[Statement], dialect: SqlDialect) -> bool {
    let foreign = |dt: &DataType| {
        dialect != SqlDialect::MySql
            && matches!(
                dt,
                DataType::Set(_) | DataType::SignedInteger | DataType::UnsignedInteger
            )
    };
    let bad_expr = |expr: &Expr| match expr {
        Expr::Function(func)
            if func.name.0.len() == 1
                && func.name.0[0]
                    .as_ident()
                    .is_some_and(|ident| ident.quote_style.is_none())
                && func.name.to_string().eq_ignore_ascii_case("CAST") =>
        {
            ControlFlow::Break(())
        }
        Expr::Cast { data_type, .. } if foreign(data_type) => ControlFlow::Break(()),
        _ => ControlFlow::Continue(()),
    };
    stmts.iter().any(|stmt| {
        visit_expressions(stmt, bad_expr).is_break()
            || matches!(stmt, Statement::CreateTable(create)
                if create.columns.iter().any(|col| foreign(&col.data_type)))
    })
}

fn parse_first(sql: &str, dialect: SqlDialect) -> Option<Statement> {
    parse_query(sql, dialect)?.into_iter().next()
}

/// Canonical spelling of a SQL type, e.g. `INTEGER` -> `INT`, `INT[]` -> `ARRAY<INT>`.
pub(crate) fn canonical_type(dt: &DataType, dialect: SqlDialect) -> String {
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
        _ => SQL_TO_PYTHON_TYPE.get(base).map(|ty| ty.to_string()),
    }
}

/// Load schema from CREATE TABLE, VIEW, DOMAIN and TYPE ... AS ENUM statements; in
/// PostgreSQL also ALTER, DROP, search_path and functions (see [`crate::catalog`]).
/// Returns the `(kind, name)` of the tables that could not be parsed even without
/// their constraints and of the views that could not be parsed.
pub fn load_schema_with_skipped(schema_sql: &str, dialect: &str) -> (Schema, Vec<SkippedObject>) {
    let (catalog, skipped) = crate::catalog::load(schema_sql, dialect);
    (Arc::new(catalog), skipped)
}

#[cfg(test)]
pub(crate) fn load_schema(schema_sql: &str, dialect: &str) -> Schema {
    load_schema_with_skipped(schema_sql, dialect).0
}

/// Output columns of `view`'s query, named by its column list, as read against `catalog`.
pub(crate) fn view_columns(
    catalog: &SchemaData,
    dialect: SqlDialect,
    view: &sqlparser::ast::CreateView,
) -> Vec<Column> {
    let analyzer = Analyzer::new(catalog, dialect);
    let mut columns = match analyzer.query_columns(&view.query) {
        Ok(Some(columns)) => columns,
        _ => vec![Column::unknown("*")],
    };
    for (column, name) in columns.iter_mut().zip(&view.columns) {
        column.name = alias_name(&name.name, dialect);
    }
    columns
}

/// `(python_type, sql_type)` of a column of SQL type `sql_type`, whose Python type is
/// `python_type` when that is not the one the SQL type maps to.
pub(crate) fn typed(sql_type: String, python_type: Option<String>, dialect: SqlDialect) -> Typed {
    match python_type {
        Some(python_type) => (Some(python_type), Some(sql_type)),
        None => from_sql_type(sql_type, dialect),
    }
}

impl Column {
    /// The Python type as written in a row type: `T | None` for a column that can be NULL,
    /// else `python_type`. `Any` already admits `None`.
    pub fn display_type(&self) -> Option<String> {
        let python_type = self.python_type.as_deref()?;
        Some(if self.nullable() && python_type != ANY {
            format!("{python_type} | None")
        } else {
            python_type.to_string()
        })
    }
}

/// Infer the Python return type for a SQL query, e.g. `tuple[int, str]`.
pub fn infer_type(sql: &str, dialect: &str, schema: &Option<Schema>) -> Inferred<Option<String>> {
    let columns = analyze_select(sql, dialect, schema)?;
    // A column of unknown type, `*` the schema cannot resolve included, leaves the row type unknown.
    Ok(columns.and_then(|columns| {
        let types: Option<Vec<String>> = columns.iter().map(Column::display_type).collect();
        let types = types?;
        Some(if types.is_empty() {
            "tuple[()]".to_string()
        } else {
            format!("tuple[{}]", types.join(", "))
        })
    }))
}

/// The result columns of a query, or `None` when it returns no rows or doesn't parse.
pub fn analyze_select(
    sql: &str,
    dialect: &str,
    schema: &Option<Schema>,
) -> Inferred<Option<Vec<Column>>> {
    analyze_with_diagnostics(sql, dialect, schema).0
}

/// [`analyze_select`], and why columns are untyped: unknown or ambiguous names, unknown
/// functions and types, constructs the analyzer does not type. Name resolution (tables,
/// columns, ambiguity) is only judged when `schema` has tables or views.
pub fn analyze_with_diagnostics(
    sql: &str,
    dialect: &str,
    schema: &Option<Schema>,
) -> (Inferred<Option<Vec<Column>>>, Vec<NameDiagnostic>) {
    let dialect = SqlDialect::from_name(dialect);
    let Some(stmt) = parse_first(sql, dialect) else {
        return (Ok(None), Vec::new());
    };
    let mut analyzer = Analyzer::new(
        schema
            .as_deref()
            .unwrap_or_else(|| crate::catalog::empty(dialect)),
        dialect,
    );
    analyzer.diag = Diagnostics::collecting();
    let columns = analyzer.statement_columns(&stmt);
    (columns, analyzer.diag.items())
}

/// Why `sql` does not parse in `dialect`, as sqlparser words it; `None` when it parses
/// (empty SQL included).
pub fn parse_error(sql: &str, dialect: &str) -> Option<String> {
    let dialect = SqlDialect::from_name(dialect);
    if parse_query(sql, dialect).is_some() {
        return None;
    }
    let input = parser_input(sql, dialect);
    Some(
        match Parser::parse_sql(dialect.parser_dialect().as_ref(), &input) {
            Ok(_) => "unsupported in this dialect".to_string(),
            Err(err) => {
                let text = err.to_string();
                text.strip_prefix("sql parser error: ")
                    .unwrap_or(&text)
                    .to_string()
            }
        },
    )
}

/// Whether `sql` yields rows: a query, DML with RETURNING, or unparseable SQL.
///
/// Empty SQL, DDL, DML without RETURNING and transaction control return no rows.
pub fn returns_rows(sql: &str, dialect: &str) -> bool {
    match parse_query(sql, SqlDialect::from_name(dialect)) {
        None => true,
        Some(stmts) => stmts.first().is_some_and(statement_returns_rows),
    }
}

/// Expand SELECT * into an explicit `col::TYPE` list, keeping other projections verbatim.
pub fn expand_star(sql: &str, dialect: &str, schema: &Option<Schema>) -> Option<String> {
    let schema = schema.as_deref().filter(|schema| !schema.is_empty())?;
    let dialect = SqlDialect::from_name(dialect);
    let stmt = parse_first(sql, dialect)?;
    let analyzer = Analyzer::new(schema, dialect);
    let Statement::Query(query) = &stmt else {
        return None;
    };
    let select = leftmost_select(&query.body)?;
    // Expansion drops table qualifiers, so with more than one source it could
    // write ambiguous columns.
    let [TableFactor::Table { name, .. }] = relations(&select.from).collect::<Vec<_>>()[..] else {
        return None;
    };
    if query.with.as_ref().is_some_and(|with| {
        with.cte_tables.iter().any(|cte| {
            cte.alias
                .name
                .value
                .eq_ignore_ascii_case(&object_name(name))
        })
    }) {
        return None;
    }
    let scope = table_scope(&select.from[0].relation, dialect);

    let upper = sql.to_ascii_uppercase();
    let select_idx = top_level(&upper, "SELECT", 0)?;
    let from_idx = top_level(&upper, "FROM", select_idx)?;

    let mut items = Vec::new();
    let mut has_star = false;
    for item in split_top_level(&sql[select_idx + "SELECT".len()..from_idx]) {
        let Some(star) = STAR_ITEM.captures(item) else {
            items.push(item.to_string());
            continue;
        };
        let reference = star.get(1).map(|m| m.as_str());
        if !analyzer.star_resolvable(&select.from, reference) {
            return None;
        }
        let columns = analyzer.schema_star_columns(reference, &scope)?;
        items.extend(
            columns
                .iter()
                .map(|col| format!("{}::{}", col.ident, col.sql_type)),
        );
        has_star = true;
    }
    has_star.then(|| {
        format!(
            "{}SELECT {} {}",
            &sql[..select_idx],
            items.join(", "),
            &sql[from_idx..]
        )
    })
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || !byte.is_ascii()
}

/// Byte index of `keyword` outside parentheses at or after `start`.
fn top_level(sql_upper: &str, keyword: &str, start: usize) -> Option<usize> {
    let bytes = sql_upper.as_bytes();
    let mut depth = 0i32;
    for i in 0..bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        let end = i + keyword.len();
        if i >= start
            && depth == 0
            && sql_upper.as_bytes().get(i..end) == Some(keyword.as_bytes())
            && (i == 0 || !is_word(bytes[i - 1]))
            && (end == bytes.len() || !is_word(bytes[end]))
        {
            return Some(i);
        }
    }
    None
}

/// Split on commas outside parentheses, trimming each item.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, byte) in text.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' if depth == 0 => {
                items.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(text[start..].trim());
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn infer(sql: &str) -> Option<String> {
        infer_type(sql, "postgres", &None).unwrap()
    }

    #[test]
    fn test_dml_returning() {
        assert_eq!(
            infer("DELETE FROM t RETURNING id::INTEGER").as_deref(),
            Some("tuple[int]")
        );
        assert_eq!(
            infer("WITH x AS (SELECT 1) UPDATE t SET a = 1 RETURNING id::INTEGER").as_deref(),
            Some("tuple[int]")
        );
        assert_eq!(infer("INSERT INTO t SELECT a::INTEGER FROM u"), None);
        assert_eq!(
            infer(
                "INSERT INTO t (a) VALUES (1) ON CONFLICT (a) WHERE b = 'x' DO NOTHING RETURNING a::INTEGER"
            )
            .as_deref(),
            Some("tuple[int]")
        );
    }

    #[test]
    fn test_empty_target_list() {
        assert_eq!(
            infer("SELECT FROM t FOR UPDATE").as_deref(),
            Some("tuple[()]")
        );
        assert_eq!(infer("SELECT FROM t").as_deref(), Some("tuple[()]"));
    }

    #[test]
    fn test_schema_order_and_canonical_types() {
        let schema = Some(load_schema(
            "CREATE TABLE users (id INTEGER NOT NULL, score NUMERIC(10,2) NOT NULL, tags TEXT[] NOT NULL)",
            "postgres",
        ));
        assert_eq!(
            expand_star("SELECT * FROM users", "postgres", &schema).as_deref(),
            Some("SELECT id::INT, score::DECIMAL(10, 2), tags::ARRAY<TEXT> FROM users")
        );
        assert_eq!(
            infer_type("SELECT * FROM users", "postgres", &schema)
                .unwrap()
                .as_deref(),
            Some("tuple[int, decimal.Decimal, list[str]]")
        );
    }

    #[test]
    fn test_parser_input() {
        assert_eq!(
            parser_input(
                "  --sql\r\nSELECT %s, %(id)s, %b, %(a-b)t, '%s', 5 %% 2",
                SqlDialect::MySql
            ),
            "SELECT NULL, NULL, NULL, NULL, 'NULL', 5 % 2"
        );
        assert_eq!(parser_input("--sql SELECT 1", SqlDialect::MySql), "");
        assert_eq!(
            parser_input("SELECT %d, %%(a)s", SqlDialect::MySql),
            "SELECT %d, %(a)s"
        );
        assert_eq!(
            parser_input(
                "SELECT 1 FOR no key update OF a, s.b FOR KEY SHARE FOR SHARE OF \"c\" NOWAIT",
                SqlDialect::Postgres
            ),
            "SELECT 1 FOR UPDATE OF a FOR SHARE FOR SHARE OF \"c\" NOWAIT"
        );
        assert_eq!(
            parser_input("SELECT 1 FOR NO KEY UPDATE OF a, b", SqlDialect::MySql),
            "SELECT 1 FOR NO KEY UPDATE OF a, b"
        );
        assert_eq!(
            parser_input(
                "INSERT INTO t (a) VALUES (1) ON CONFLICT (a, lower(b)) WHERE c = ')DO(' OR d = 'x''DO' DO NOTHING",
                SqlDialect::Postgres
            ),
            "INSERT INTO t (a) VALUES (1) ON CONFLICT (a, lower(b)) DO NOTHING"
        );
        assert_eq!(
            parser_input("SELECT 1 ON CONFLICT (a) WHERE b", SqlDialect::Postgres),
            "SELECT 1 ON CONFLICT (a) WHERE b"
        );
        assert_eq!(
            infer_type("--sql\nSELECT 1 AS x", "mysql", &None)
                .unwrap()
                .as_deref(),
            Some("tuple[int]")
        );
        // Schema DDL is not a query: a `%s` there is not a placeholder.
        assert!(load_schema("CREATE TABLE t (a INT DEFAULT %s)", "postgres").is_empty());
    }

    fn names(sql: &str) -> Vec<String> {
        names_in(sql, "postgres")
    }

    #[test]
    fn test_unaliased_names() {
        assert_eq!(
            names("SELECT NOT active, -n, -1, n::INT, COUNT(*), lower(x), f(x) FROM t"),
            ["?column?", "?column?", "?column?", "n", "count", "lower", "f"]
        );
    }

    #[test]
    fn test_mysql_column_types() {
        let schema = Some(load_schema(
            "CREATE TABLE t (a INT3 ZEROFILL, b INT ZEROFILL UNSIGNED, \
             c INT SIGNED ZEROFILL, d MIDDLEINT(8) UNSIGNED)",
            "mysql",
        ));
        assert_eq!(
            expand_star("SELECT * FROM t", "mysql", &schema).as_deref(),
            Some("SELECT a::UMEDIUMINT, b::UINT, c::UINT, d::UMEDIUMINT(8) FROM t")
        );
        assert_eq!(
            names_in("SELECT 1 zerofill", "mysql"),
            ["zerofill"],
            "queries are not respelled"
        );
    }

    fn names_in(sql: &str, dialect: &str) -> Vec<String> {
        analyze_select(sql, dialect, &None)
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|col| col.name)
            .collect()
    }

    #[test]
    fn test_numbered_placeholders() {
        assert_eq!(
            numbered_placeholders(
                "SELECT '?', \"a?\", b ?| c, d ?& e FROM t WHERE x = ? -- ?\nAND y = ?"
            ),
            "SELECT '?', \"a?\", b ?| c, d ?& e FROM t WHERE x = $1 -- ?\nAND y = $2"
        );
    }

    #[test]
    fn test_question_mark_placeholders() {
        assert_eq!(
            infer("SELECT a::INT FROM t WHERE id = ? AND b ?| array['x']").as_deref(),
            Some("tuple[int]")
        );
        assert_eq!(
            infer("SELECT a::INT FROM t WHERE b ? 'x'").as_deref(),
            Some("tuple[int]")
        );
        assert_eq!(infer("SELECT a::INT FROM t WHERE b ? 'x' AND id = ?"), None);
    }

    #[test]
    fn test_dialect_type_aliases() {
        let expand = |columns: &str, dialect: &str| {
            let schema = Some(load_schema(&format!("CREATE TABLE t ({columns})"), dialect));
            expand_star("SELECT * FROM t", dialect, &schema)
        };
        assert_eq!(
            expand("a INT64, b FLOAT64", "bigquery").as_deref(),
            Some("SELECT a::BIGINT, b::DOUBLE FROM t")
        );
        assert_eq!(
            expand("a Int8, b INT8", "clickhouse").as_deref(),
            Some("SELECT a::TINYINT, b::TINYINT FROM t")
        );
        assert_eq!(
            expand("a INT8", "postgres").as_deref(),
            Some("SELECT a::BIGINT FROM t")
        );
        assert_eq!(
            expand("a UNIQUEIDENTIFIER", "tsql").as_deref(),
            Some("SELECT a::UUID FROM t")
        );
    }

    #[test]
    fn test_sum_of_unknown_schema_column_is_an_error() {
        let schema = Some(load_schema("CREATE TABLE users (id INT)", "postgres"));
        assert!(infer_type("SELECT SUM(nope) FROM users", "postgres", &schema).is_err());
    }

    #[test]
    fn test_ctes_and_derived_tables() {
        let schema = Some(load_schema(
            "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, born DATE NOT NULL)",
            "postgres",
        ));
        let infer = |sql: &str| infer_type(sql, "postgres", &schema).unwrap();
        assert_eq!(
            infer("WITH r AS (SELECT id, name FROM users) SELECT id, name FROM r").as_deref(),
            Some("tuple[int, str]")
        );
        assert_eq!(
            infer("WITH r(a, b) AS (SELECT id, name FROM users) SELECT b, a FROM r").as_deref(),
            Some("tuple[str, int]")
        );
        assert_eq!(
            infer("WITH a AS (SELECT id FROM users), b AS (SELECT id FROM a) SELECT * FROM b")
                .as_deref(),
            Some("tuple[int]")
        );
        assert_eq!(
            infer("SELECT t.* FROM (SELECT name, born FROM users) t").as_deref(),
            Some("tuple[str, datetime.date]")
        );
        assert_eq!(
            infer("SELECT x FROM (SELECT id FROM users) t(x)").as_deref(),
            Some("tuple[int]")
        );
        assert_eq!(
            infer("SELECT u.id, o.name FROM users u JOIN LATERAL (SELECT name FROM users WHERE id = u.id) o ON true")
                .as_deref(),
            Some("tuple[int, str]")
        );
        assert_eq!(
            infer("WITH n AS (INSERT INTO users (name) VALUES ('a') RETURNING id, name) SELECT id, name FROM n")
                .as_deref(),
            Some("tuple[int, str]")
        );
        assert_eq!(
            infer("WITH RECURSIVE c AS (SELECT id FROM users UNION ALL SELECT id FROM c) SELECT id FROM c")
                .as_deref(),
            Some("tuple[int]")
        );
        assert_eq!(
            infer("WITH users AS (SELECT 1 AS other) SELECT id FROM users"),
            None
        );
    }

    #[test]
    fn test_set_operation_types() {
        let schema = Some(load_schema(
            "CREATE TABLE a (n INTEGER, t TEXT, d DATE);\
             CREATE TABLE b (n BIGINT, t VARCHAR(5), d TIMESTAMP);\
             CREATE TABLE c (g GEOMETRY);",
            "postgres",
        ));
        let sql_types = |sql: &str| -> Vec<Option<String>> {
            Analyzer::new(schema.as_deref().unwrap(), SqlDialect::Postgres)
                .statement_columns(&parse_first(sql, SqlDialect::Postgres).unwrap())
                .unwrap()
                .unwrap()
                .into_iter()
                .map(|col| col.sql_type)
                .collect()
        };
        let types = |names: &[&str]| -> Vec<Option<String>> {
            names
                .iter()
                .map(|name| (!name.is_empty()).then(|| name.to_string()))
                .collect()
        };
        assert_eq!(
            sql_types("SELECT n, t, d FROM a UNION SELECT n, t, d FROM b"),
            types(&["BIGINT", "TEXT", "TIMESTAMP"])
        );
        assert_eq!(
            sql_types("SELECT NULL, $1, n FROM a INTERSECT SELECT n, t, n::REAL FROM b"),
            types(&["BIGINT", "VARCHAR(5)", "FLOAT"])
        );
        assert_eq!(
            sql_types("SELECT n, t FROM a EXCEPT (SELECT t, n FROM b UNION SELECT n, t FROM b)"),
            types(&["", ""])
        );
        assert_eq!(
            sql_types("SELECT (SELECT n FROM a UNION ALL SELECT n FROM b)"),
            types(&["BIGINT"])
        );
        assert_eq!(
            sql_types(
                "WITH RECURSIVE r AS (SELECT n FROM a UNION ALL SELECT n * 2::BIGINT FROM r) \
                 SELECT n FROM r"
            ),
            types(&["BIGINT"])
        );
        assert_eq!(
            sql_types("SELECT g, ARRAY[g] FROM c UNION SELECT n, ARRAY[g] FROM a, c"),
            types(&["", "ARRAY<GEOMETRY>"])
        );
        assert_eq!(
            sql_types("SELECT ARRAY[n], n * 1.5 FROM a UNION SELECT ARRAY[n], n FROM b"),
            types(&["ARRAY<BIGINT>", "DECIMAL"])
        );
    }
    /// `(rule, message)` pairs for a query against a two-table schema.
    fn diagnose(sql: &str) -> Vec<(&'static str, String)> {
        let schema = Some(load_schema(
            "CREATE TABLE a (id INT, n INT, g GEOMETRY); CREATE TABLE b (id INT, t TEXT);",
            "postgres",
        ));
        analyze_with_diagnostics(sql, "postgres", &schema)
            .1
            .into_iter()
            .map(|found| (found.rule.name(), found.message))
            .collect()
    }

    fn rules(sql: &str) -> Vec<&'static str> {
        diagnose(sql).into_iter().map(|(rule, _)| rule).collect()
    }

    #[test]
    fn test_name_diagnostics() {
        assert_eq!(rules("SELECT nope FROM a"), ["unknown-column"]);
        assert_eq!(rules("SELECT x.id FROM a"), ["unknown-table"]);
        assert_eq!(rules("SELECT id FROM a, b"), ["ambiguous-column"]);
        assert_eq!(rules("SELECT id FROM missing"), ["unknown-table"]);
        assert_eq!(rules("SELECT frob(n) FROM a"), ["unknown-function"]);
        assert_eq!(rules("SELECT n::mood FROM a"), ["unknown-type"]);
        assert_eq!(rules("SELECT g FROM a"), ["unknown-type"]);
        assert_eq!(rules("SELECT 1 UNION TABLE a"), ["unsupported-syntax"]);
    }

    #[test]
    fn test_name_diagnostics_stay_quiet() {
        for sql in [
            "SELECT id FROM a JOIN b USING (id)",
            "SELECT id FROM a NATURAL JOIN b",
            "WITH c AS (SELECT n AS k FROM a) SELECT k FROM c",
            "SELECT s.k FROM (SELECT n AS k FROM a) s",
            "SELECT (SELECT max(b.id) FROM b WHERE b.id = a.id) FROM a",
            "SELECT a.id, l.m FROM a, LATERAL (SELECT max(b.id) AS m FROM b WHERE b.id = a.id) l",
            "SELECT A.ID, \"n\" FROM A",
            "SELECT NULL, $1",
        ] {
            assert!(rules(sql).is_empty(), "{sql}: {:?}", diagnose(sql));
        }
        // A table function's columns are not read, which is reported, not guessed.
        assert_eq!(
            rules("SELECT x FROM generate_series(1, 3) AS g(x)"),
            ["unsupported-syntax"]
        );
    }

    #[test]
    fn test_name_rules_need_a_schema() {
        let found = analyze_with_diagnostics("SELECT nope, id FROM a, b", "postgres", &None).1;
        assert!(found.is_empty(), "{found:?}");
    }
}
