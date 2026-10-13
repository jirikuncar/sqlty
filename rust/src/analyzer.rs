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
    visit_expressions, ArrayElemTypeDef, DataType, Expr, FromTable, FunctionArg, FunctionArgExpr,
    FunctionArguments, Join, JoinConstraint, JoinOperator, ObjectName, OutputClause, Query, Select,
    SelectItem, SelectItemQualifiedWildcardKind, SetExpr, Statement, TableAlias, TableFactor,
    TableObject, TableWithJoins, Value,
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

/// Result types of functions by name (COUNT, AVG, SUM, MIN, MAX and COALESCE are special-cased).
static FUNCTION_TYPES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let groups: [(&[&str], &str); 7] = [
        (
            &[
                "LENGTH",
                "POSITION",
                "CHAR_LENGTH",
                "BIT_LENGTH",
                "OCTET_LENGTH",
                "ROW_NUMBER",
                "RANK",
                "DENSE_RANK",
                "NTILE",
            ],
            "int",
        ),
        (
            &[
                "STDDEV",
                "STDDEV_POP",
                "STDDEV_SAMP",
                "VARIANCE",
                "VAR_POP",
                "VAR_SAMP",
            ],
            "float",
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
            "str",
        ),
        (&["NOW", "CURRENT_TIMESTAMP"], "datetime.datetime"),
        (&["CURRENT_DATE"], "datetime.date"),
        (&["CURRENT_TIME"], "datetime.time"),
        (
            &[
                "JSON_BUILD_OBJECT",
                "JSONB_BUILD_OBJECT",
                "TO_JSON",
                "TO_JSONB",
                "JSON_AGG",
                "JSONB_AGG",
                "JSONB_OBJECT_AGG",
                "JSON_OBJECT_AGG",
            ],
            "dict",
        ),
    ];
    groups
        .into_iter()
        .flat_map(|(names, ty)| names.iter().map(move |name| (*name, ty)))
        .collect()
});

static STAR_ITEM: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?:(\w+)\s*\.\s*)?\*$").unwrap());

/// What a schema defines; shared, so caching it is cheap.
#[derive(Default)]
pub struct SchemaData {
    /// Table name -> columns in declaration order.
    tables: HashMap<String, Vec<SchemaColumn>>,
    /// View name -> output columns, as analyzed when the view was created.
    views: HashMap<String, Vec<Column>>,
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
}

impl SchemaColumn {
    fn typed(&self) -> Typed {
        from_sql_type(self.sql_type.clone())
    }
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
        POSTGRES_LOCK_FIXES
            .iter()
            .fold(sql.into_owned(), |sql, (pattern, replacement)| {
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

/// [`load_schema`], and the names of the tables that could not be parsed even
/// without their constraints. Tables load first, then each view in order, so a view
/// may read the views before it.
pub fn load_schema_with_skipped(schema_sql: &str, dialect: &str) -> (Schema, Vec<String>) {
    let dialect = SqlDialect::from_name(dialect);
    let mut skipped = Vec::new();
    let statements = ddl::split_statements(schema_sql, dialect == SqlDialect::MySql);
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
                .map(|col| SchemaColumn {
                    name: col.name.value.to_lowercase(),
                    ident: match col.name.quote_style {
                        Some(_) => col.name.to_string(),
                        None => col.name.value.to_lowercase(),
                    },
                    sql_type: canonical_type(&col.data_type, dialect),
                })
                .collect();
            (object_name(&create.name), columns)
        })
        .collect();
    let mut data = SchemaData {
        tables,
        views: HashMap::new(),
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
        format!("tuple[{}]", types.join(", "))
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

fn first_arg(args: &FunctionArguments) -> Option<&Expr> {
    match args {
        FunctionArguments::List(list) => match list.args.first()? {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => Some(expr),
            _ => None,
        },
        _ => None,
    }
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

/// Functions whose result type is known, besides those in [`FUNCTION_TYPES`].
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
                        .map(non_empty)
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
                Ok(from_sql_type(canonical_type(data_type, self.dialect)))
            }
            Expr::Nested(inner) => self.expr(inner, scope),
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
            Expr::Exists { .. } | Expr::InSubquery { .. } => known("bool"),
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
                let arg = first_arg(&func.args);
                match name.as_str() {
                    "COUNT" => Ok((Some("int".to_string()), Some("INTEGER".to_string()))),
                    "AVG" => known("float"),
                    "SUM" | "MIN" | "MAX" => self.aggregate(&name, arg, scope),
                    "COALESCE" => arg.map_or(Ok((None, None)), |arg| self.expr(arg, scope)),
                    _ => Ok((
                        FUNCTION_TYPES.get(name.as_str()).map(|ty| ty.to_string()),
                        None,
                    )),
                }
            }
            _ => Ok((None, None)),
        }
    }

    /// SUM/MIN/MAX take their argument's type; SUM falls back to `NUMERIC`.
    fn aggregate(&self, name: &str, arg: Option<&Expr>, scope: &Scope) -> Inferred<Typed> {
        let typed = arg.map_or(Ok((None, None)), |arg| self.expr(arg, scope))?;
        let numeric = || Ok((Some("float".to_string()), Some("NUMERIC".to_string())));
        if typed.0.is_some() || name != "SUM" {
            return Ok(typed);
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
