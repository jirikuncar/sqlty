//! SQL type inference with sqlparser-rs.
//!
//! Rows come from the leftmost `SELECT` of a query or from the top-level
//! `RETURNING` of INSERT/UPDATE/DELETE/MERGE. Type names are canonicalized
//! (`INTEGER` becomes `INT`, `NUMERIC(10,2)` becomes `DECIMAL(10, 2)`), as
//! `SELECT *` expansion writes them back into queries.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;

use crate::ddl;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use sqlparser::ast::{
    visit_expressions, ArrayElemTypeDef, BinaryOperator, DataType, Expr, FromTable, FunctionArg,
    FunctionArgExpr, FunctionArguments, Join, JoinConstraint, JoinOperator, ObjectName,
    OutputClause, Query, Select, SelectItem, SelectItemQualifiedWildcardKind, SetExpr, Statement,
    TableAlias, TableFactor, TableObject, TableWithJoins, UnaryOperator,
    UserDefinedTypeRepresentation, Value,
};
use sqlparser::dialect::{
    dialect_from_str, ClickHouseDialect, GenericDialect, MySqlDialect, PostgreSqlDialect,
    SQLiteDialect,
};
use sqlparser::parser::Parser;

/// Mapping from canonical SQL type names to Python type strings.
static SQL_TO_PYTHON_TYPE: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
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
        ("DECIMAL", "float"),
        ("NUMERIC", "float"),
        ("UDOUBLE", "float"),
        ("UDECIMAL", "float"),
        ("BIGDECIMAL", "float"),
        ("TEXT", "str"),
        ("VARCHAR", "str"),
        ("CHAR", "str"),
        ("STRING", "str"),
        ("BOOLEAN", "bool"),
        ("BOOL", "bool"),
        ("DATE", "datetime.date"),
        ("TIMESTAMP", "datetime.datetime"),
        ("TIMESTAMPTZ", "datetime.datetime"),
        ("DATETIME", "datetime.datetime"),
        ("TIME", "datetime.time"),
        ("TIMETZ", "datetime.time"),
        ("INTERVAL", "datetime.timedelta"),
        ("ARRAY", "list"),
        ("JSON", "dict"),
        ("JSONB", "dict"),
        ("UUID", "uuid.UUID"),
        ("BYTEA", "bytes"),
        ("BLOB", "bytes"),
        ("BINARY", "bytes"),
        ("VARBINARY", "bytes"),
    ])
});

/// How a function's result type follows from its arguments (indexes count from 0).
#[derive(Clone, Copy)]
enum Returns {
    /// A fixed canonical SQL type.
    Sql(&'static str),
    /// `void`: Python `None`.
    Void,
    /// The type of an argument.
    Arg(usize),
    /// The element type of an array argument.
    Elem(usize),
    /// An array of an argument's type.
    ArrayOf(usize),
    /// `lower`/`upper`: the element type of a range argument, else `TEXT`.
    Bound,
    /// `AVG` and friends: `DOUBLE` for floats, `INTERVAL` for intervals, else `NUMERIC`.
    Mean,
    /// `sqrt`, `sign`, ...: `DOUBLE` for floats and integers (PostgreSQL prefers
    /// `double precision`), `NUMERIC` for numerics.
    Real,
    /// `round`, `trunc`, `log`: [`Returns::Real`], except `NUMERIC` when called with a
    /// scale or base (a second argument).
    Scaled,
    /// `date_trunc`: a timestamp argument's type, `TIMESTAMPTZ` for a date.
    Trunc(usize),
}

use Returns::{Arg, Elem, Sql};

/// Functions of every dialect, by upper-case name (COUNT, SUM, MIN, MAX and COALESCE are
/// special-cased).
const COMMON_FUNCTIONS: &[(&[&str], Returns)] = &[
    (
        &[
            "LENGTH",
            "POSITION",
            "CHAR_LENGTH",
            "BIT_LENGTH",
            "OCTET_LENGTH",
            "NTILE",
        ],
        Sql("INT"),
    ),
    (&["ROW_NUMBER", "RANK", "DENSE_RANK"], Sql("BIGINT")),
    (&["AVG"], Returns::Mean),
    (
        &[
            "STDDEV",
            "STDDEV_POP",
            "STDDEV_SAMP",
            "VARIANCE",
            "VAR_POP",
            "VAR_SAMP",
        ],
        Sql("DOUBLE"),
    ),
    (
        &[
            "UPPER",
            "LOWER",
            "TRIM",
            "CONCAT",
            "SUBSTR",
            "SUBSTRING",
            "REPLACE",
            "STRING_AGG",
            "GROUP_CONCAT",
        ],
        Sql("TEXT"),
    ),
    (&["NOW", "CURRENT_TIMESTAMP"], Sql("TIMESTAMPTZ")),
    (&["CURRENT_DATE"], Sql("DATE")),
    (&["CURRENT_TIME"], Sql("TIMETZ")),
    (
        &[
            "JSON_BUILD_OBJECT",
            "TO_JSON",
            "JSON_AGG",
            "JSON_OBJECT_AGG",
        ],
        Sql("JSON"),
    ),
    (
        &[
            "JSONB_BUILD_OBJECT",
            "TO_JSONB",
            "JSONB_AGG",
            "JSONB_OBJECT_AGG",
        ],
        Sql("JSONB"),
    ),
];

/// PostgreSQL built-ins, from the "Functions and Operators" chapter of the manual; in
/// PostgreSQL they take precedence over [`COMMON_FUNCTIONS`].
const POSTGRES_FUNCTIONS: &[(&[&str], Returns)] = &[
    (
        &[
            "CURRENT_SETTING",
            "SET_CONFIG",
            "LEFT",
            "RIGHT",
            "LPAD",
            "RPAD",
            "LTRIM",
            "RTRIM",
            "BTRIM",
            "INITCAP",
            "REPEAT",
            "REVERSE",
            "TRANSLATE",
            "OVERLAY",
            "CONCAT_WS",
            "FORMAT",
            "MD5",
            "TO_CHAR",
            "TO_HEX",
            "CHR",
            "QUOTE_IDENT",
            "QUOTE_LITERAL",
            "QUOTE_NULLABLE",
            "SPLIT_PART",
            "REGEXP_REPLACE",
            "ENCODE",
            "CONVERT_FROM",
            "ARRAY_TO_STRING",
            "PG_SIZE_PRETTY",
            "VERSION",
            "CURRENT_DATABASE",
            "CURRENT_SCHEMA",
            "CURRENT_USER",
            "SESSION_USER",
            "USER",
            "PG_TYPEOF",
            "JSONB_TYPEOF",
            "JSON_TYPEOF",
            "JSONB_PRETTY",
            "JSONB_EXTRACT_PATH_TEXT",
            "JSON_EXTRACT_PATH_TEXT",
            "TS_HEADLINE",
            "JSONB_ARRAY_ELEMENTS_TEXT",
            "JSON_ARRAY_ELEMENTS_TEXT",
        ],
        Sql("TEXT"),
    ),
    (
        &[
            "ASCII",
            "STRPOS",
            "ARRAY_LENGTH",
            "ARRAY_NDIMS",
            "ARRAY_POSITION",
            "CARDINALITY",
            "JSONB_ARRAY_LENGTH",
            "JSON_ARRAY_LENGTH",
            "WIDTH_BUCKET",
            "PG_BACKEND_PID",
            "NUM_NONNULLS",
            "NUM_NULLS",
        ],
        Sql("INT"),
    ),
    (
        &[
            "NEXTVAL",
            "CURRVAL",
            "LASTVAL",
            "SETVAL",
            "TXID_CURRENT",
            "PG_DATABASE_SIZE",
            "PG_TOTAL_RELATION_SIZE",
            "PG_RELATION_SIZE",
            "PG_TABLE_SIZE",
            "PG_INDEXES_SIZE",
        ],
        Sql("BIGINT"),
    ),
    (
        &[
            "RANDOM",
            "DATE_PART",
            "PI",
            "DEGREES",
            "RADIANS",
            "SIN",
            "COS",
            "TAN",
            "ASIN",
            "ACOS",
            "ATAN",
            "ATAN2",
        ],
        Sql("DOUBLE"),
    ),
    (&["TS_RANK", "TS_RANK_CD"], Sql("FLOAT")),
    (&["TO_NUMBER"], Sql("DECIMAL")),
    (
        &[
            "STARTS_WITH",
            "REGEXP_LIKE",
            "PG_TRY_ADVISORY_LOCK",
            "PG_TRY_ADVISORY_XACT_LOCK",
            "PG_TRY_ADVISORY_LOCK_SHARED",
            "PG_TRY_ADVISORY_XACT_LOCK_SHARED",
            "PG_ADVISORY_UNLOCK",
            "PG_ADVISORY_UNLOCK_SHARED",
            "PG_IS_IN_RECOVERY",
            "PG_TERMINATE_BACKEND",
            "PG_CANCEL_BACKEND",
            "ISFINITE",
            "BOOL_AND",
            "BOOL_OR",
            "EVERY",
            "LOWER_INF",
            "UPPER_INF",
            "ISEMPTY",
            "JSONB_PATH_EXISTS",
            "JSON_VALID",
        ],
        Sql("BOOLEAN"),
    ),
    (
        &[
            "PG_ADVISORY_LOCK",
            "PG_ADVISORY_XACT_LOCK",
            "PG_ADVISORY_LOCK_SHARED",
            "PG_ADVISORY_XACT_LOCK_SHARED",
            "PG_ADVISORY_UNLOCK_ALL",
            "PG_NOTIFY",
            "PG_SLEEP",
            "SETSEED",
        ],
        Returns::Void,
    ),
    (
        &[
            "TRANSACTION_TIMESTAMP",
            "STATEMENT_TIMESTAMP",
            "CLOCK_TIMESTAMP",
            "TO_TIMESTAMP",
            "MAKE_TIMESTAMPTZ",
        ],
        Sql("TIMESTAMPTZ"),
    ),
    (&["LOCALTIMESTAMP", "MAKE_TIMESTAMP"], Sql("TIMESTAMP")),
    (&["TO_DATE", "MAKE_DATE"], Sql("DATE")),
    (&["LOCALTIME", "MAKE_TIME"], Sql("TIME")),
    (
        &[
            "AGE",
            "JUSTIFY_DAYS",
            "JUSTIFY_HOURS",
            "JUSTIFY_INTERVAL",
            "MAKE_INTERVAL",
        ],
        Sql("INTERVAL"),
    ),
    (
        &["GEN_RANDOM_UUID", "UUID_GENERATE_V4", "UUID_GENERATE_V1"],
        Sql("UUID"),
    ),
    (
        &[
            "SHA224",
            "SHA256",
            "SHA384",
            "SHA512",
            "DECODE",
            "GEN_RANDOM_BYTES",
            "CONVERT_TO",
            "DIGEST",
        ],
        Sql("VARBINARY"),
    ),
    (
        &["STRING_TO_ARRAY", "REGEXP_SPLIT_TO_ARRAY"],
        Sql("ARRAY<TEXT>"),
    ),
    (
        &[
            "ROW_TO_JSON",
            "JSON_BUILD_ARRAY",
            "JSON_OBJECT",
            "JSON_STRIP_NULLS",
            "JSON_EXTRACT_PATH",
            "JSON_ARRAY_ELEMENTS",
        ],
        Sql("JSON"),
    ),
    (
        &[
            "JSONB_BUILD_ARRAY",
            "JSONB_OBJECT",
            "JSONB_STRIP_NULLS",
            "JSONB_SET",
            "JSONB_INSERT",
            "JSONB_EXTRACT_PATH",
            "JSONB_ARRAY_ELEMENTS",
            "JSONB_PATH_QUERY",
            "JSONB_PATH_QUERY_FIRST",
        ],
        Sql("JSONB"),
    ),
    (&["TO_TSVECTOR", "SETWEIGHT"], Sql("TSVECTOR")),
    (
        &[
            "TO_TSQUERY",
            "PLAINTO_TSQUERY",
            "PHRASETO_TSQUERY",
            "WEBSEARCH_TO_TSQUERY",
        ],
        Sql("TSQUERY"),
    ),
    (&["INT4RANGE"], Sql("INT4RANGE")),
    (&["INT8RANGE"], Sql("INT8RANGE")),
    (&["NUMRANGE"], Sql("NUMRANGE")),
    (&["TSRANGE"], Sql("TSRANGE")),
    (&["TSTZRANGE"], Sql("TSTZRANGE")),
    (&["DATERANGE"], Sql("DATERANGE")),
    (
        &[
            "ABS",
            "MOD",
            "GREATEST",
            "LEAST",
            "NULLIF",
            "FIRST_VALUE",
            "LAST_VALUE",
            "NTH_VALUE",
            "LAG",
            "LEAD",
            "ANY_VALUE",
            "ARRAY_APPEND",
            "ARRAY_CAT",
            "ARRAY_REMOVE",
            "ARRAY_REPLACE",
            "GENERATE_SERIES",
        ],
        Arg(0),
    ),
    (&["ARRAY_PREPEND"], Arg(1)),
    (&["UNNEST"], Elem(0)),
    (&["ARRAY_AGG"], Returns::ArrayOf(0)),
    (&["LOWER", "UPPER"], Returns::Bound),
    (
        &[
            "STDDEV",
            "STDDEV_POP",
            "STDDEV_SAMP",
            "VARIANCE",
            "VAR_POP",
            "VAR_SAMP",
        ],
        Returns::Mean,
    ),
    (
        &[
            "CEIL", "CEILING", "FLOOR", "SQRT", "CBRT", "EXP", "LN", "POWER", "SIGN",
        ],
        Returns::Real,
    ),
    (&["ROUND", "TRUNC", "LOG"], Returns::Scaled),
    (&["DATE_TRUNC"], Returns::Trunc(1)),
];

fn function_table(
    entries: &'static [(&[&'static str], Returns)],
) -> HashMap<&'static str, Returns> {
    entries
        .iter()
        .flat_map(|(names, returns)| names.iter().map(move |name| (*name, *returns)))
        .collect()
}

static FUNCTION_TYPES: Lazy<HashMap<&'static str, Returns>> =
    Lazy::new(|| function_table(COMMON_FUNCTIONS));
static POSTGRES_FUNCTION_TYPES: Lazy<HashMap<&'static str, Returns>> =
    Lazy::new(|| function_table(POSTGRES_FUNCTIONS));

static STAR_ITEM: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?:(\w+)\s*\.\s*)?\*$").unwrap());

/// What a schema defines; shared, so caching it is cheap.
#[derive(Default)]
pub struct SchemaData {
    /// Table name -> columns in declaration order.
    tables: HashMap<String, Vec<SchemaColumn>>,
    /// View name -> output columns, as analyzed when the view was created.
    views: HashMap<String, Vec<Column>>,
    /// Domain and enum type names -> the types their columns have.
    types: HashMap<String, UserType>,
}

pub type Schema = Arc<SchemaData>;

impl SchemaData {
    fn get(&self, table: &str) -> Option<&Vec<SchemaColumn>> {
        self.tables.get(table)
    }

    fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.views.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct SchemaColumn {
    /// Lowercased name, for lookups and result column names.
    pub name: String,
    /// The name as SQL spells it, quotes kept, for `SELECT *` expansion.
    pub ident: String,
    pub sql_type: String,
    /// The Python type when it is not the one `sql_type` maps to (an enum's `Literal`).
    python_type: Option<String>,
}

impl SchemaColumn {
    fn typed(&self) -> Typed {
        match &self.python_type {
            Some(python_type) => (Some(python_type.clone()), Some(self.sql_type.clone())),
            None => from_sql_type(self.sql_type.clone()),
        }
    }
}

/// A domain or enum: the SQL type its columns show (a domain's base type, an enum's
/// own name) and the Python type when that is a `Literal` of the enum's labels.
#[derive(Clone, Debug)]
struct UserType {
    sql_type: String,
    python_type: Option<String>,
}

/// A domain or enum as written in the schema, before domains over other types resolve.
enum TypeDef {
    Domain(DataType),
    Enum(Vec<String>),
}

/// `(reference, table)` pairs for the tables a statement reads, in query order.
type Scope = Vec<(String, String)>;

/// Inference fails outright only for `SUM(column)` of a column missing from a schema table.
type Inferred<T> = Result<T, String>;

/// A dialect name, matched case-insensitively; names sqlparser doesn't know read as generic SQL.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SqlDialect {
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
    fn from_name(name: &str) -> Self {
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

    fn parser_dialect(self) -> Box<dyn sqlparser::dialect::Dialect> {
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
static SQL_MARKER: Lazy<Regex> = Lazy::new(|| Regex::new(r"\A\s*--sql[^\n]*\n?").unwrap());

/// PostgreSQL row-lock clauses sqlparser cannot read, respelled as the closest ones it can:
/// `NO KEY UPDATE` as `UPDATE`, `KEY SHARE` as `SHARE`, and `OF` keeps only its first table.
static POSTGRES_LOCK_FIXES: Lazy<[(Regex, &str); 3]> = Lazy::new(|| {
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
static PLACEHOLDER: Lazy<Regex> = Lazy::new(|| Regex::new(r"%%|%(?:\([^)]+\))?[sbt]").unwrap());

/// A `?` placeholder in group 1, or a literal, quoted name, comment or `?|`/`?&` operator to skip.
static QMARK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"'(?:[^']|'')*'|"(?:[^"]|"")*"|--[^\n]*|(?s:/\*.*?\*/)|\?[|&]|(\?)"#).unwrap()
});

/// MySQL column types sqlparser rejects: `ZEROFILL` (which implies `UNSIGNED`), then
/// `INT3`/`MIDDLEINT` before `UNSIGNED`.
static MYSQL_TYPE_FIXES: Lazy<[(Regex, &str); 2]> = Lazy::new(|| {
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
fn parse_ddl(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
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
    static CONFLICT_TARGET: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)\bON\s+CONFLICT\s*\(").unwrap());
    static WHERE_CLAUSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\A\s*WHERE\b").unwrap());
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
fn canonical_type(dt: &DataType, dialect: SqlDialect) -> String {
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

/// Map a canonical SQL type to a Python type string; unknown types are `str`.
fn python_type(sql_type: &str) -> String {
    let base = sql_type.split(['(', '<']).next().unwrap_or_default().trim();
    SQL_TO_PYTHON_TYPE.get(base).unwrap_or(&"str").to_string()
}

/// Lowercased unqualified name: `public.users` -> `users`.
fn object_name(name: &ObjectName) -> String {
    name.0
        .last()
        .and_then(|part| part.as_ident())
        .map(|ident| ident.value.to_lowercase())
        .unwrap_or_default()
}

/// Load schema from CREATE TABLE, VIEW, DOMAIN and TYPE ... AS ENUM statements.
#[cfg(test)]
pub fn load_schema(schema_sql: &str, dialect: &str) -> Schema {
    load_schema_with_skipped(schema_sql, dialect).0
}

/// The labels of an enum as a `typing.Literal`.
fn literal_of(labels: &[String]) -> String {
    let labels: Vec<String> = labels
        .iter()
        .map(|label| format!("'{}'", label.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect();
    format!("typing.Literal[{}]", labels.join(", "))
}

/// The lowercased unqualified name of a user-defined type such as `logfire.percent`.
fn custom_name(dt: &DataType) -> Option<String> {
    match dt {
        DataType::Custom(name, modifiers) if modifiers.is_empty() => Some(object_name(name)),
        _ => None,
    }
}

/// The type a domain or enum named `name` gives its columns; domains over domains
/// resolve through `defs`, up to a fixed depth.
fn resolve_user_type(
    name: &str,
    defs: &HashMap<String, TypeDef>,
    dialect: SqlDialect,
    depth: usize,
) -> Option<UserType> {
    match defs.get(name)? {
        TypeDef::Enum(labels) => Some(UserType {
            sql_type: name.to_uppercase(),
            python_type: (!labels.is_empty()).then(|| literal_of(labels)),
        }),
        TypeDef::Domain(base) => Some(
            custom_name(base)
                .filter(|_| depth < 16)
                .and_then(|base| resolve_user_type(&base, defs, dialect, depth + 1))
                .unwrap_or_else(|| UserType {
                    sql_type: canonical_type(base, dialect),
                    python_type: None,
                }),
        ),
    }
}

/// The SQL type of a column, and its Python type when a domain or enum sets it.
fn resolve_type(
    dt: &DataType,
    types: &HashMap<String, UserType>,
    dialect: SqlDialect,
) -> (String, Option<String>) {
    match custom_name(dt).and_then(|name| types.get(&name)) {
        Some(user) => (user.sql_type.clone(), user.python_type.clone()),
        None => (canonical_type(dt, dialect), None),
    }
}

fn type_defs(statements: &[ddl::Statement], dialect: SqlDialect) -> HashMap<String, TypeDef> {
    let mut defs = HashMap::new();
    let parser_dialect = dialect.parser_dialect();
    let parse_with = |text: &str| Parser::new(parser_dialect.as_ref()).try_with_sql(text).ok();
    for stmt in statements {
        if stmt.creates("domain") {
            let parsed = stmt.domain().and_then(|(name, base)| {
                let name = parse_with(&name)?.parse_object_name(false).ok()?;
                let base = parse_with(&base)?.parse_data_type().ok()?;
                Some((object_name(&name), base))
            });
            if let Some((name, base)) = parsed {
                defs.insert(name, TypeDef::Domain(base));
            }
        } else if stmt.creates("type") {
            if let Some(Statement::CreateType {
                name,
                representation: Some(UserDefinedTypeRepresentation::Enum { labels }),
            }) = parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next())
            {
                let labels = labels.into_iter().map(|label| label.value).collect();
                defs.insert(object_name(&name), TypeDef::Enum(labels));
            }
        }
    }
    defs
}

/// [`load_schema`], and the names of the tables that could not be parsed even
/// without their constraints. Domains and enums load first, then tables, then each
/// view in order, so a view may read the views before it.
pub fn load_schema_with_skipped(schema_sql: &str, dialect: &str) -> (Schema, Vec<String>) {
    let dialect = SqlDialect::from_name(dialect);
    let mut skipped = Vec::new();
    let statements = ddl::split_statements(schema_sql, dialect == SqlDialect::MySql);
    let defs = type_defs(&statements, dialect);
    let types: HashMap<String, UserType> = defs
        .keys()
        .filter_map(|name| Some((name.clone(), resolve_user_type(name, &defs, dialect, 0)?)))
        .collect();
    let tables = statements
        .iter()
        .filter(|stmt| stmt.is_create_table())
        .filter_map(|stmt| {
            let parsed = parse_ddl(&stmt.sql(), dialect)
                .or_else(|| parse_ddl(&stmt.without_constraints()?, dialect));
            if parsed.is_none() {
                skipped.push(stmt.table_name());
            }
            parsed?.into_iter().next()
        })
        .filter_map(|stmt| match stmt {
            Statement::CreateTable(create) if !create.columns.is_empty() => Some(create),
            _ => None,
        })
        .map(|create| {
            let columns = create
                .columns
                .iter()
                .filter(|col| col.data_type != DataType::Unspecified)
                .map(|col| {
                    let (sql_type, python_type) = resolve_type(&col.data_type, &types, dialect);
                    SchemaColumn {
                        name: col.name.value.to_lowercase(),
                        ident: match col.name.quote_style {
                            Some(_) => col.name.to_string(),
                            None => col.name.value.to_lowercase(),
                        },
                        sql_type,
                        python_type,
                    }
                })
                .collect();
            (object_name(&create.name), columns)
        })
        .collect();
    let mut data = SchemaData {
        tables,
        views: HashMap::new(),
        types,
    };
    for stmt in statements.iter().filter(|stmt| stmt.creates("view")) {
        let Some(Statement::CreateView(view)) =
            parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next())
        else {
            continue;
        };
        let analyzer = Analyzer {
            schema: &data,
            dialect,
            virtuals: HashMap::new(),
        };
        let mut columns = match analyzer.query_columns(&view.query) {
            Ok(Some(columns)) => columns,
            _ => vec![Column::unknown("*")],
        };
        for (column, name) in columns.iter_mut().zip(&view.columns) {
            column.name = name.name.value.clone();
        }
        data.views.insert(object_name(&view.name), columns);
    }
    (Arc::new(data), skipped)
}

/// Infer the Python return type for a SQL query, e.g. `tuple[int, str]`.
pub fn infer_type(sql: &str, dialect: &str, schema: &Option<Schema>) -> Inferred<Option<String>> {
    let columns = analyze_select(sql, dialect, schema)?;
    // A `*` the schema cannot resolve leaves the row type unknown.
    let columns = columns.filter(|columns| !columns.iter().any(Column::is_unknown_star));
    Ok(columns.map(|columns| {
        let types: Vec<&str> = columns
            .iter()
            .map(|col| col.python_type.as_deref().unwrap_or("str"))
            .collect();
        if types.is_empty() {
            "tuple[()]".to_string()
        } else {
            format!("tuple[{}]", types.join(", "))
        }
    }))
}

/// The result columns of a query, or `None` when it returns no rows or doesn't parse.
pub fn analyze_select(
    sql: &str,
    dialect: &str,
    schema: &Option<Schema>,
) -> Inferred<Option<Vec<Column>>> {
    let empty = SchemaData::default();
    let dialect = SqlDialect::from_name(dialect);
    let Some(stmt) = parse_first(sql, dialect) else {
        return Ok(None);
    };
    let analyzer = Analyzer {
        schema: schema.as_deref().unwrap_or(&empty),
        dialect,
        virtuals: HashMap::new(),
    };
    analyzer.statement_columns(&stmt)
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

fn statement_returns_rows(stmt: &Statement) -> bool {
    match stmt {
        Statement::Query(query) => match query.body.as_ref() {
            SetExpr::Insert(inner)
            | SetExpr::Update(inner)
            | SetExpr::Delete(inner)
            | SetExpr::Merge(inner) => statement_returns_rows(inner),
            _ => true,
        },
        _ => returning(stmt).is_some(),
    }
}

/// The non-empty top-level RETURNING list of INSERT/UPDATE/DELETE/MERGE.
fn returning(stmt: &Statement) -> Option<&[SelectItem]> {
    let items = match stmt {
        Statement::Insert(insert) => &insert.returning,
        Statement::Update(update) => &update.returning,
        Statement::Delete(delete) => &delete.returning,
        Statement::Merge(merge) => {
            return match &merge.output {
                Some(OutputClause::Returning { select_items, .. }) if !select_items.is_empty() => {
                    Some(select_items)
                }
                _ => None,
            };
        }
        _ => return None,
    };
    items.as_deref().filter(|items| !items.is_empty())
}

/// Tables a DML statement writes to, for resolving RETURNING columns.
fn dml_scope(stmt: &Statement) -> Scope {
    match stmt {
        Statement::Insert(insert) => match &insert.table {
            TableObject::TableName(name) => vec![(object_name(name), object_name(name))],
            _ => Vec::new(),
        },
        Statement::Update(update) => table_scope(&update.table.relation),
        Statement::Merge(merge) => table_scope(&merge.table),
        Statement::Delete(delete) => match &delete.from {
            FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => tables
                .first()
                .map(|table| table_scope(&table.relation))
                .unwrap_or_default(),
        },
        _ => Vec::new(),
    }
}

fn table_scope(factor: &TableFactor) -> Scope {
    match factor {
        TableFactor::Table { name, alias, .. } => {
            let table = object_name(name);
            let reference = alias
                .as_ref()
                .map(|alias| alias.name.value.to_lowercase())
                .unwrap_or_else(|| table.clone());
            vec![(reference, table)]
        }
        _ => Vec::new(),
    }
}

/// Sources of a SELECT: each FROM relation followed by its joins (`FROM a, b` counts two),
/// looking into unaliased parentheses such as `FROM (a JOIN b ON ...)`.
fn relations(from: &[TableWithJoins]) -> std::vec::IntoIter<&TableFactor> {
    fn push<'a>(table: &'a TableWithJoins, into: &mut Vec<&'a TableFactor>) {
        for factor in
            std::iter::once(&table.relation).chain(table.joins.iter().map(|j| &j.relation))
        {
            match factor {
                TableFactor::NestedJoin {
                    table_with_joins,
                    alias: None,
                } => push(table_with_joins, into),
                other => into.push(other),
            }
        }
    }
    let mut factors = Vec::new();
    for table in from {
        push(table, &mut factors);
    }
    factors.into_iter()
}

/// Whether any join in `from`, nested ones included, merges columns.
fn any_merging_join(from: &[TableWithJoins]) -> bool {
    from.iter().any(|table| {
        table.joins.iter().any(merges_columns)
            || match &table.relation {
                TableFactor::NestedJoin {
                    table_with_joins,
                    alias: None,
                } => any_merging_join(std::slice::from_ref(table_with_joins)),
                _ => false,
            }
    })
}

/// The name a relation is referred to by: its alias, else its table name.
fn relation_name(factor: &TableFactor) -> Option<String> {
    let alias = match factor {
        TableFactor::Table { name, alias, .. } => {
            return Some(alias.as_ref().map_or_else(
                || object_name(name),
                |alias| alias.name.value.to_lowercase(),
            ))
        }
        TableFactor::Derived { alias, .. } => alias.as_ref(),
        _ => None,
    };
    alias.map(|alias| alias.name.value.to_lowercase())
}

/// Whether a join merges same-named columns (`USING`, `NATURAL`), so `*` does not list them all.
fn merges_columns(join: &Join) -> bool {
    use JoinOperator::*;
    let constraint = match &join.join_operator {
        Join(c)
        | Inner(c)
        | Left(c)
        | LeftOuter(c)
        | Right(c)
        | RightOuter(c)
        | FullOuter(c)
        | CrossJoin(c)
        | Semi(c)
        | LeftSemi(c)
        | RightSemi(c)
        | Anti(c)
        | LeftAnti(c)
        | RightAnti(c)
        | StraightJoin(c)
        | AsOf { constraint: c, .. } => c,
        _ => return false,
    };
    matches!(
        constraint,
        JoinConstraint::Using(_) | JoinConstraint::Natural
    )
}

/// The table an alias (or table name) refers to.
fn resolve(reference: &str, scope: &Scope) -> String {
    let reference = reference.to_lowercase();
    scope
        .iter()
        .find(|(alias, _)| *alias == reference)
        .map(|(_, table)| table.clone())
        .unwrap_or(reference)
}

/// The SELECT whose columns a query returns: the left arm of a UNION, unparenthesized.
fn leftmost_select(body: &SetExpr) -> Option<&Select> {
    match body {
        SetExpr::Select(select) => Some(select),
        SetExpr::SetOperation { left, .. } => leftmost_select(left),
        SetExpr::Query(query) => leftmost_select(&query.body),
        _ => None,
    }
}

/// The leading unnamed expression arguments of a call (`*` and named arguments end the list).
fn positional_args(args: &FunctionArguments) -> Vec<&Expr> {
    match args {
        FunctionArguments::List(list) => list
            .args
            .iter()
            .map_while(|arg| match arg {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => Some(expr),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `INT` from `ARRAY<INT>` or `ARRAY<INT>[3]`.
fn array_element(sql_type: &str) -> Option<&str> {
    let inner = sql_type.strip_prefix("ARRAY<")?;
    Some(&inner[..inner.rfind('>')?])
}

/// The element type of a range type such as `TSTZRANGE`.
fn range_element(sql_type: &str) -> Option<&'static str> {
    Some(match type_base(sql_type) {
        "INT4RANGE" => "INT",
        "INT8RANGE" => "BIGINT",
        "NUMRANGE" => "DECIMAL",
        "TSRANGE" => "TIMESTAMP",
        "TSTZRANGE" => "TIMESTAMPTZ",
        "DATERANGE" => "DATE",
        _ => return None,
    })
}

/// A result column: name, Python type and SQL type; unknown types are `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub python_type: Option<String>,
    pub sql_type: Option<String>,
}

impl Column {
    fn unknown(name: &str) -> Self {
        Column {
            name: name.to_string(),
            python_type: None,
            sql_type: None,
        }
    }

    fn is_unknown_star(&self) -> bool {
        self.name == "*" && self.python_type.is_none()
    }
}

/// `(python_type, sql_type)` of an expression.
type Typed = (Option<String>, Option<String>);

fn known(python_type: &str) -> Inferred<Typed> {
    Ok((Some(python_type.to_string()), None))
}

fn from_sql_type(sql_type: String) -> Typed {
    (Some(python_type(&sql_type)), Some(sql_type))
}

/// Functions whose result type is known, besides those in [`COMMON_FUNCTIONS`].
const TYPED_FUNCTIONS: [&str; 6] = ["COUNT", "AVG", "SUM", "MIN", "MAX", "COALESCE"];

/// Typed functions whose unaliased column is still named after the function.
const NAMED_FUNCTIONS: [&str; 7] = [
    "OCTET_LENGTH",
    "JSON_BUILD_OBJECT",
    "JSONB_BUILD_OBJECT",
    "TO_JSON",
    "TO_JSONB",
    "JSONB_AGG",
    "JSONB_OBJECT_AGG",
];

/// Name of an unaliased result column: a column's name, a (signed) literal's text, `*` for
/// `COUNT(*)`, the function's name for a call sqlty can't type; empty for anything else.
fn column_name(expr: &Expr) -> String {
    match expr {
        Expr::Identifier(ident) => ident.value.clone(),
        Expr::CompoundIdentifier(parts) => parts
            .last()
            .map(|part| part.value.clone())
            .unwrap_or_default(),
        Expr::Cast { expr, .. } => column_name(expr),
        Expr::UnaryOp { expr, .. } if matches!(**expr, Expr::Value(_)) => column_name(expr),
        Expr::Extract { field, .. } => field.to_string(),
        Expr::Value(value) => match &value.value {
            Value::Number(number, _) => number.clone(),
            Value::Null => "NULL".to_string(),
            Value::Boolean(_) | Value::Placeholder(_) => String::new(),
            other => other.clone().into_string().unwrap_or_default(),
        },
        Expr::Function(func) => {
            let name = func.name.to_string().to_uppercase();
            let star = matches!(&func.args, FunctionArguments::List(list)
                if matches!(list.args.first(), Some(FunctionArg::Unnamed(FunctionArgExpr::Wildcard))));
            let typed = TYPED_FUNCTIONS.contains(&name.as_str())
                || FUNCTION_TYPES.contains_key(name.as_str());
            if name == "COUNT" && star {
                "*".to_string()
            } else if typed && !NAMED_FUNCTIONS.contains(&name.as_str()) {
                String::new()
            } else {
                func.name
                    .0
                    .last()
                    .and_then(|part| part.as_ident())
                    .map(|ident| ident.value.clone())
                    .unwrap_or_default()
            }
        }
        _ => String::new(),
    }
}

/// Output columns of the CTEs and derived tables in scope, by table key: a CTE's
/// name, or `(alias)` for a derived table.
type Virtuals = HashMap<String, Vec<Column>>;

#[derive(Clone)]
struct Analyzer<'a> {
    schema: &'a SchemaData,
    dialect: SqlDialect,
    virtuals: Virtuals,
}

impl<'a> Analyzer<'a> {
    /// Columns of a CTE, derived table or schema view; a CTE hides a view of its name.
    fn virtual_columns(&self, table: &str) -> Option<&Vec<Column>> {
        self.virtuals
            .get(table)
            .or_else(|| self.schema.views.get(table))
    }

    /// Whether a table reference reads a virtual source: a CTE or view by its bare
    /// name, or a view by its schema-qualified name.
    fn is_virtual(&self, name: &ObjectName) -> bool {
        let table = object_name(name);
        if name.0.len() == 1 {
            self.virtual_columns(&table).is_some()
        } else {
            self.schema.views.contains_key(&table)
        }
    }

    fn statement_columns(&self, stmt: &Statement) -> Inferred<Option<Vec<Column>>> {
        match stmt {
            Statement::Query(query) => self.query_columns(query),
            _ => match returning(stmt) {
                Some(items) => self
                    .projection(items, &dml_scope(stmt), None)
                    .map(non_empty),
                None => Ok(None),
            },
        }
    }

    fn query_columns(&self, query: &Query) -> Inferred<Option<Vec<Column>>> {
        if let Some(with) = &query.with {
            let mut inner = self.clone();
            for cte in &with.cte_tables {
                let mut columns = inner
                    .query_columns(&cte.query)?
                    .unwrap_or_else(|| vec![Column::unknown("*")]);
                rename_columns(&mut columns, &cte.alias);
                inner
                    .virtuals
                    .insert(cte.alias.name.value.to_lowercase(), columns);
            }
            return inner.query_body(query);
        }
        self.query_body(query)
    }

    fn query_body(&self, query: &Query) -> Inferred<Option<Vec<Column>>> {
        match query.body.as_ref() {
            SetExpr::Insert(stmt)
            | SetExpr::Update(stmt)
            | SetExpr::Delete(stmt)
            | SetExpr::Merge(stmt) => self.statement_columns(stmt),
            body => match leftmost_select(body) {
                Some(select) => {
                    let (analyzer, scope) = self.scope_of(&select.from)?;
                    analyzer
                        .projection(&select.projection, &scope, Some(&select.from))
                        .map(Some)
                }
                None => Ok(None),
            },
        }
    }

    /// Scope of a FROM clause, and an analyzer that knows its derived tables. A
    /// `LATERAL` subquery is analyzed with the relations before it.
    fn scope_of(&self, from: &[TableWithJoins]) -> Inferred<(Analyzer<'a>, Scope)> {
        let mut analyzer = self.clone();
        let mut scope = Scope::new();
        for factor in relations(from) {
            match factor {
                TableFactor::Table { name, alias, .. } if analyzer.is_virtual(name) => {
                    let table = object_name(name);
                    let reference = alias
                        .as_ref()
                        .map_or_else(|| table.clone(), |alias| alias.name.value.to_lowercase());
                    scope.push((reference, table));
                }
                TableFactor::Derived {
                    subquery,
                    alias: Some(alias),
                    ..
                } => {
                    let mut columns = analyzer
                        .query_columns(subquery)?
                        .unwrap_or_else(|| vec![Column::unknown("*")]);
                    rename_columns(&mut columns, alias);
                    let reference = alias.name.value.to_lowercase();
                    let table = format!("({reference})");
                    analyzer.virtuals.insert(table.clone(), columns);
                    scope.push((reference, table));
                }
                other => scope.extend(table_scope(other)),
            }
        }
        Ok((analyzer, scope))
    }

    /// One column per result column; a star may yield several, or one unknown `*`
    /// column when the schema cannot resolve it. `from` is the SELECT's FROM clause.
    fn projection(
        &self,
        items: &[SelectItem],
        scope: &Scope,
        from: Option<&[TableWithJoins]>,
    ) -> Inferred<Vec<Column>> {
        let mut columns = Vec::new();
        for item in items {
            let (expr, name) = match item {
                SelectItem::UnnamedExpr(expr) => (expr, column_name(expr)),
                SelectItem::ExprWithAlias { expr, alias } => (expr, alias.value.clone()),
                SelectItem::ExprWithAliases { expr, aliases } => (
                    expr,
                    aliases
                        .first()
                        .map_or_else(|| column_name(expr), |a| a.value.clone()),
                ),
                SelectItem::Wildcard(_) => {
                    columns.extend(self.star(None, scope, from));
                    continue;
                }
                SelectItem::QualifiedWildcard(
                    SelectItemQualifiedWildcardKind::ObjectName(name),
                    _,
                ) => {
                    columns.extend(self.star(Some(&object_name(name)), scope, from));
                    continue;
                }
                SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::Expr(_), _) => {
                    columns.push(Column::unknown("*"));
                    continue;
                }
            };
            let (python_type, sql_type) = self.expr(expr, scope)?;
            columns.push(Column {
                name,
                python_type,
                sql_type,
            });
        }
        Ok(columns)
    }

    /// Columns of a star from the schema, or a single unknown `*` column.
    fn star(
        &self,
        reference: Option<&str>,
        scope: &Scope,
        from: Option<&[TableWithJoins]>,
    ) -> Vec<Column> {
        let resolvable = from.is_none_or(|from| self.star_resolvable(from, reference));
        match resolvable
            .then(|| self.star_columns(reference, scope))
            .flatten()
        {
            Some(columns) => columns,
            None => vec![Column::unknown("*")],
        }
    }

    /// Whether `*` (or `t.*`) reads only sources with known columns: schema tables, and
    /// CTEs and derived tables whose own columns are known.
    ///
    /// `USING`/`NATURAL` joins merge columns, so their stars are unresolved.
    fn star_resolvable(&self, from: &[TableWithJoins], reference: Option<&str>) -> bool {
        if reference.is_none() && any_merging_join(from) {
            return false;
        }
        let reference = reference.map(str::to_lowercase);
        let mut matched = relations(from)
            .filter(|factor| reference.is_none() || relation_name(factor) == reference)
            .peekable();
        matched.peek().is_some()
            && matched.all(|factor| match factor {
                TableFactor::Table { name, .. } => self
                    .virtual_columns(&object_name(name))
                    .is_none_or(|columns| !columns.iter().any(Column::is_unknown_star)),
                TableFactor::Derived {
                    alias: Some(alias), ..
                } => self
                    .virtuals
                    .get(&format!("({})", alias.name.value.to_lowercase()))
                    .is_some_and(|columns| !columns.iter().any(Column::is_unknown_star)),
                _ => false,
            })
    }

    /// Tables a star reads: the one `reference` names, or every scope table.
    fn star_tables(reference: Option<&str>, scope: &Scope) -> Vec<String> {
        match reference {
            Some(reference) => vec![resolve(reference, scope)],
            None => scope.iter().map(|(_, table)| table.clone()).collect(),
        }
    }

    /// Expand `*` (all scope tables) or `t.*` from the sources, or `None` if unknown.
    fn star_columns(&self, reference: Option<&str>, scope: &Scope) -> Option<Vec<Column>> {
        let tables = Self::star_tables(reference, scope);
        if tables.is_empty() {
            return None;
        }
        let columns = tables
            .iter()
            .map(|table| {
                self.virtual_columns(table).cloned().or_else(|| {
                    self.schema.get(table).map(|columns| {
                        columns
                            .iter()
                            .map(|col| {
                                let (python_type, sql_type) = col.typed();
                                Column {
                                    name: col.name.clone(),
                                    python_type,
                                    sql_type,
                                }
                            })
                            .collect()
                    })
                })
            })
            .collect::<Option<Vec<Vec<Column>>>>()?;
        Some(columns.into_iter().flatten().collect())
    }

    /// Expand `*` or `t.*` into schema columns, as `SELECT *` rewriting writes them.
    fn schema_star_columns(
        &self,
        reference: Option<&str>,
        scope: &Scope,
    ) -> Option<Vec<SchemaColumn>> {
        let tables = Self::star_tables(reference, scope);
        if tables.is_empty() {
            return None;
        }
        let columns = tables
            .iter()
            .map(|table| self.schema.get(table))
            .collect::<Option<Vec<_>>>()?;
        Some(columns.into_iter().flatten().cloned().collect())
    }

    /// Type of a column: its qualifier's source, else the first scope source having it.
    fn column_type(&self, expr: &Expr, scope: &Scope) -> Option<Typed> {
        let lookup = |table: &str, column: &str| {
            if let Some(columns) = self.virtual_columns(table) {
                return columns
                    .iter()
                    .find(|col| col.name.to_lowercase() == column)
                    .map(|col| (col.python_type.clone(), col.sql_type.clone()));
            }
            self.schema
                .get(table)?
                .iter()
                .find(|col| col.name == column)
                .map(|col| col.typed())
        };
        match expr {
            Expr::Identifier(ident) => {
                let column = ident.value.to_lowercase();
                scope.iter().find_map(|(_, table)| lookup(table, &column))
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let table = resolve(&parts[parts.len() - 2].value, scope);
                lookup(&table, &parts[parts.len() - 1].value.to_lowercase())
            }
            _ => None,
        }
    }

    fn expr(&self, expr: &Expr, scope: &Scope) -> Inferred<Typed> {
        match expr {
            Expr::Cast { data_type, .. } => {
                let (sql_type, python_type) =
                    resolve_type(data_type, &self.schema.types, self.dialect);
                Ok(match python_type {
                    Some(python_type) => (Some(python_type), Some(sql_type)),
                    None => from_sql_type(sql_type),
                })
            }
            Expr::Nested(inner) => self.expr(inner, scope),
            Expr::UnaryOp {
                op: UnaryOperator::Minus | UnaryOperator::Plus,
                expr: inner,
            } => self.expr(inner, scope),
            Expr::Ceil { expr: inner, .. } | Expr::Floor { expr: inner, .. } => {
                self.function_type(Returns::Real, &[inner], scope)
            }
            Expr::Extract { .. } if self.dialect == SqlDialect::Postgres => {
                Ok(from_sql_type("DECIMAL".to_string()))
            }
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => {
                Ok(self.column_type(expr, scope).unwrap_or((None, None)))
            }
            Expr::Value(value) => match &value.value {
                Value::Number(number, _) if number.contains(['.', 'e', 'E']) => known("float"),
                Value::Number(..) => known("int"),
                Value::Boolean(_) => known("bool"),
                Value::Null | Value::Placeholder(_) => Ok((None, None)),
                _ => known("str"),
            },
            Expr::Exists { .. }
            | Expr::InSubquery { .. }
            | Expr::InList { .. }
            | Expr::InUnnest { .. }
            | Expr::Between { .. }
            | Expr::Like { .. }
            | Expr::ILike { .. }
            | Expr::SimilarTo { .. }
            | Expr::RLike { .. }
            | Expr::AnyOp { .. }
            | Expr::AllOp { .. }
            | Expr::IsNull(_)
            | Expr::IsNotNull(_)
            | Expr::IsTrue(_)
            | Expr::IsNotTrue(_)
            | Expr::IsFalse(_)
            | Expr::IsNotFalse(_)
            | Expr::IsUnknown(_)
            | Expr::IsNotUnknown(_)
            | Expr::IsDistinctFrom(..)
            | Expr::IsNotDistinctFrom(..)
            | Expr::UnaryOp {
                op: UnaryOperator::Not,
                ..
            } => Ok(from_sql_type("BOOLEAN".to_string())),
            Expr::Interval(_) => Ok(from_sql_type("INTERVAL".to_string())),
            Expr::BinaryOp { left, op, right } => self.binary_op(left, op, right, scope),
            Expr::Trim { .. } | Expr::Substring { .. } => known("str"),
            Expr::Position { .. } => known("int"),
            Expr::Subquery(query) => match leftmost_select(&query.body) {
                Some(select) if !select.projection.is_empty() => {
                    let (analyzer, scope) = self.scope_of(&select.from)?;
                    Ok(analyzer
                        .projection(&select.projection[..1], &scope, Some(&select.from))?
                        .into_iter()
                        .next()
                        .map_or((None, None), |col| (col.python_type, col.sql_type)))
                }
                _ => Ok((None, None)),
            },
            Expr::Case {
                conditions,
                else_result,
                ..
            } => {
                match conditions
                    .first()
                    .map(|when| &when.result)
                    .or(else_result.as_deref())
                {
                    Some(branch) => self.expr(branch, scope),
                    None => Ok((None, None)),
                }
            }
            Expr::Function(func) => {
                let name = func.name.to_string().to_uppercase();
                let args = positional_args(&func.args);
                let arg = args.first().copied();
                match name.as_str() {
                    "COUNT" => Ok((Some("int".to_string()), Some("INTEGER".to_string()))),
                    "SUM" | "MIN" | "MAX" => self.aggregate(&name, arg, scope),
                    "COALESCE" => arg.map_or(Ok((None, None)), |arg| self.expr(arg, scope)),
                    _ => match self.returns(&name) {
                        Some(returns) => self.function_type(returns, &args, scope),
                        None => Ok((None, None)),
                    },
                }
            }
            _ => Ok((None, None)),
        }
    }

    fn returns(&self, name: &str) -> Option<Returns> {
        let postgres = (self.dialect == SqlDialect::Postgres)
            .then(|| POSTGRES_FUNCTION_TYPES.get(name))
            .flatten();
        postgres.or_else(|| FUNCTION_TYPES.get(name)).copied()
    }

    /// Result type of a call to a function that [`Returns`] describes, given its arguments.
    fn function_type(&self, returns: Returns, args: &[&Expr], scope: &Scope) -> Inferred<Typed> {
        let arg = |i: usize| match args.get(i) {
            Some(arg) => self.expr(arg, scope),
            None => Ok((None, None)),
        };
        let sql = |name: &str| Ok(from_sql_type(name.to_string()));
        match returns {
            Returns::Sql(name) => sql(name),
            Returns::Void => Ok((Some("None".to_string()), Some("VOID".to_string()))),
            Returns::Arg(i) => arg(i),
            Returns::Elem(i) => Ok(arg(i)?
                .1
                .as_deref()
                .and_then(array_element)
                .map_or((None, None), |elem| from_sql_type(elem.to_string()))),
            Returns::ArrayOf(i) => {
                let typed = arg(i)?;
                let elem = typed
                    .1
                    .clone()
                    .or_else(|| operand_type(&typed).map(String::from));
                Ok(elem.map_or((None, None), |elem| from_sql_type(format!("ARRAY<{elem}>"))))
            }
            Returns::Bound => {
                let typed = arg(0)?;
                match typed.1.as_deref().and_then(range_element) {
                    Some(elem) => sql(elem),
                    None => sql("TEXT"),
                }
            }
            Returns::Mean => {
                let typed = arg(0)?;
                match operand_type(&typed) {
                    Some("DOUBLE" | "FLOAT") => sql("DOUBLE"),
                    Some("INTERVAL") => sql("INTERVAL"),
                    Some(ty) if numeric_rank(ty).is_some_and(|rank| rank <= 3) => sql("DECIMAL"),
                    _ => known("float"),
                }
            }
            Returns::Real | Returns::Scaled => {
                let typed = arg(0)?;
                match operand_type(&typed) {
                    _ if matches!(returns, Returns::Scaled) && args.len() > 1 => sql("DECIMAL"),
                    Some("DOUBLE" | "FLOAT") => sql("DOUBLE"),
                    Some(ty) if numeric_rank(ty).is_some_and(|rank| rank <= 2) => sql("DOUBLE"),
                    Some("DECIMAL" | "NUMERIC") => sql("DECIMAL"),
                    _ => Ok((None, None)),
                }
            }
            Returns::Trunc(i) => {
                let typed = arg(i)?;
                match operand_type(&typed) {
                    Some("DATE") => sql("TIMESTAMPTZ"),
                    Some("TIMESTAMP" | "TIMESTAMPTZ" | "INTERVAL") => Ok(typed),
                    _ => Ok((None, None)),
                }
            }
        }
    }

    /// Result type of `left op right`: predicates are `BOOLEAN`, text and JSON operators
    /// give `TEXT` or `JSONB`, arithmetic promotes its operands.
    fn binary_op(
        &self,
        left: &Expr,
        op: &BinaryOperator,
        right: &Expr,
        scope: &Scope,
    ) -> Inferred<Typed> {
        use BinaryOperator::*;
        let sql = |name: &str| Ok(from_sql_type(name.to_string()));
        match op {
            Gt | Lt | GtEq | LtEq | Spaceship | Eq | NotEq | And | Or | Xor | Match | Regexp
            | Glob | PGOverlap | PGRegexMatch | PGRegexIMatch | PGRegexNotMatch
            | PGRegexNotIMatch | PGLikeMatch | PGILikeMatch | PGNotLikeMatch | PGNotILikeMatch
            | PGStartsWith | AtAt | AtArrow | ArrowAt | AtQuestion | Question | QuestionAnd
            | QuestionPipe | Overlaps => sql("BOOLEAN"),
            LongArrow | HashLongArrow => sql("TEXT"),
            Arrow | HashArrow | HashMinus if self.dialect == SqlDialect::Postgres => {
                let json = self.expr(left, scope)?.1.is_some_and(|ty| ty == "JSON");
                sql(if json && *op != HashMinus {
                    "JSON"
                } else {
                    "JSONB"
                })
            }
            StringConcat if self.dialect != SqlDialect::MySql => {
                let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
                if l.1.as_deref() == Some("JSONB") || r.1.as_deref() == Some("JSONB") {
                    sql("JSONB")
                } else if is_text(&l) || is_text(&r) {
                    sql("TEXT")
                } else {
                    Ok((None, None))
                }
            }
            PGExp => {
                let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
                let numeric = |t: &Typed| matches!(operand_type(t), Some("DECIMAL" | "NUMERIC"));
                sql(if numeric(&l) && numeric(&r) {
                    "DECIMAL"
                } else {
                    "DOUBLE"
                })
            }
            Plus | Minus | Multiply | Divide | Modulo => {
                let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
                let mysql_division = self.dialect == SqlDialect::MySql && *op == Divide;
                let wild = |expr: &Expr| matches!(expr, Expr::Value(v) if matches!(v.value, Value::Null | Value::Placeholder(_)));
                let (l, r) = match (wild(left), wild(right)) {
                    (true, false) => (r.clone(), r),
                    (false, true) => (l.clone(), l),
                    _ => (l, r),
                };
                Ok(arithmetic(op, &l, &r, mysql_division).map_or((None, None), from_sql_type))
            }
            _ => Ok((None, None)),
        }
    }

    /// SUM/MIN/MAX take their argument's type; SUM falls back to `NUMERIC`.
    fn aggregate(&self, name: &str, arg: Option<&Expr>, scope: &Scope) -> Inferred<Typed> {
        let typed = arg.map_or(Ok((None, None)), |arg| self.expr(arg, scope))?;
        let numeric = || Ok((Some("float".to_string()), Some("NUMERIC".to_string())));
        if typed.0.is_some() || name != "SUM" {
            return Ok(if name == "SUM" && self.dialect == SqlDialect::Postgres {
                sum_type(typed)
            } else {
                typed
            });
        }
        if scope.is_empty() {
            return numeric();
        }
        let (table, column) = match arg {
            Some(Expr::Identifier(ident)) => (scope[0].1.clone(), &ident.value),
            Some(Expr::CompoundIdentifier(parts)) if parts.len() >= 2 => (
                resolve(&parts[parts.len() - 2].value, scope),
                &parts[parts.len() - 1].value,
            ),
            _ => return numeric(),
        };
        if self.schema.get(&table).is_some() && self.virtual_columns(&table).is_none() {
            return Err(format!(
                "Column '{column}' not found in table '{table}' in schema. \
                 If you want to query columns not in the schema, use an explicit cast: \
                 SUM({column}::TYPE)"
            ));
        }
        numeric()
    }
}

/// PostgreSQL's `SUM` widens: `BIGINT` for `SMALLINT`/`INT`, `NUMERIC` for `BIGINT`/`NUMERIC`.
fn sum_type(typed: Typed) -> Typed {
    let widened = match operand_type(&typed) {
        Some("SMALLINT" | "INT") => "BIGINT",
        Some("BIGINT" | "DECIMAL" | "NUMERIC") => "DECIMAL",
        Some("FLOAT") => "FLOAT",
        Some("DOUBLE") => "DOUBLE",
        _ => return typed,
    };
    from_sql_type(widened.to_string())
}

fn type_base(sql_type: &str) -> &str {
    sql_type.split(['(', '<']).next().unwrap_or_default().trim()
}

/// The SQL type of an operand; literals know only their Python type.
fn operand_type(typed: &Typed) -> Option<&str> {
    match (typed.1.as_deref(), typed.0.as_deref()) {
        (Some(sql_type), _) => Some(type_base(sql_type)),
        (None, Some("int")) => Some("INT"),
        (None, Some("float")) => Some("DOUBLE"),
        _ => None,
    }
}

fn is_text(typed: &Typed) -> bool {
    match &typed.1 {
        Some(sql_type) => matches!(type_base(sql_type), "TEXT" | "VARCHAR" | "CHAR"),
        None => typed.0.as_deref() == Some("str"),
    }
}

/// Numeric types in promotion order, with the canonical name of each rank.
const NUMERIC_RANKS: [&[&str]; 6] = [
    &["SMALLINT", "TINYINT"],
    &["INT", "MEDIUMINT"],
    &["BIGINT"],
    &["DECIMAL", "NUMERIC"],
    &["FLOAT", "REAL"],
    &["DOUBLE"],
];

fn numeric_rank(sql_type: &str) -> Option<usize> {
    NUMERIC_RANKS
        .iter()
        .position(|names| names.contains(&sql_type))
}

/// Result type of `left op right` for numbers (promoted to the wider type), and for dates,
/// timestamps and intervals; a placeholder or `NULL` operand takes the other's type.
fn arithmetic(
    op: &BinaryOperator,
    left: &Typed,
    right: &Typed,
    mysql_division: bool,
) -> Option<String> {
    use BinaryOperator::*;
    let (l, r) = (operand_type(left), operand_type(right));
    let temporal = |ty: Option<&str>| matches!(ty, Some("DATE" | "TIMESTAMP" | "TIMESTAMPTZ"));
    match (l, r) {
        (Some(a), Some(b)) if numeric_rank(a).is_some() && numeric_rank(b).is_some() => {
            let rank = numeric_rank(a)?.max(numeric_rank(b)?);
            let rank = if mysql_division { rank.max(3) } else { rank };
            Some(NUMERIC_RANKS[rank][0].to_string())
        }
        (Some(a), Some("INTERVAL")) if temporal(Some(a)) && matches!(op, Plus | Minus) => {
            Some(if a == "DATE" { "TIMESTAMP" } else { a }.to_string())
        }
        (Some("INTERVAL"), Some(b)) if temporal(Some(b)) && *op == Plus => {
            Some(if b == "DATE" { "TIMESTAMP" } else { b }.to_string())
        }
        (Some("DATE"), Some("INT" | "SMALLINT")) if matches!(op, Plus | Minus) => {
            Some("DATE".to_string())
        }
        (Some(a), Some(b))
            if *op == Minus && a == b && matches!(a, "TIMESTAMP" | "TIMESTAMPTZ") =>
        {
            Some("INTERVAL".to_string())
        }
        (Some("DATE"), Some("DATE")) if *op == Minus => Some("INT".to_string()),
        (Some("INTERVAL"), Some("INTERVAL")) if matches!(op, Plus | Minus) => {
            Some("INTERVAL".to_string())
        }
        (Some("INTERVAL"), Some(n)) | (Some(n), Some("INTERVAL"))
            if *op == Multiply && numeric_rank(n).is_some() =>
        {
            Some("INTERVAL".to_string())
        }
        _ => None,
    }
}

/// Apply a column alias list (`t(a, b)`) to a derived table's columns, by position.
fn rename_columns(columns: &mut [Column], alias: &TableAlias) {
    for (column, name) in columns.iter_mut().zip(&alias.columns) {
        column.name = name.name.value.clone();
    }
}

fn non_empty(columns: Vec<Column>) -> Option<Vec<Column>> {
    (!columns.is_empty()).then_some(columns)
}

/// Expand SELECT * into an explicit `col::TYPE` list, keeping other projections verbatim.
pub fn expand_star(sql: &str, dialect: &str, schema: &Option<Schema>) -> Option<String> {
    let schema = schema.as_deref().filter(|schema| !schema.is_empty())?;
    let dialect = SqlDialect::from_name(dialect);
    let stmt = parse_first(sql, dialect)?;
    let analyzer = Analyzer {
        schema,
        dialect,
        virtuals: HashMap::new(),
    };
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
    let scope = table_scope(&select.from[0].relation);

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
            "CREATE TABLE users (id INTEGER, score NUMERIC(10,2), tags TEXT[])",
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
            Some("tuple[int, float, list]")
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
            ["", "", "1", "n", "*", "", "f"]
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
            "CREATE TABLE users (id INTEGER, name TEXT, born DATE)",
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
            infer("WITH users AS (SELECT 1 AS other) SELECT id FROM users").as_deref(),
            Some("tuple[str]")
        );
    }
}
