//! SQL type inference, mirroring `sqlty._sql_analyzer` (the sqlglot-based Python backend).
//!
//! Rows come from the leftmost `SELECT` of a query or from the top-level
//! `RETURNING` of INSERT/UPDATE/DELETE. Types are spelled the way sqlglot
//! spells them (`INTEGER` becomes `INT`, `NUMERIC(10,2)` becomes `DECIMAL(10, 2)`)
//! so that schema-driven output matches between backends.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;
use sqlparser::ast::{
    ArrayElemTypeDef, DataType, Expr, FromTable, FunctionArg, FunctionArgExpr, FunctionArguments,
    ObjectName, OutputClause, Query, Select, SelectItem, SelectItemQualifiedWildcardKind, SetExpr,
    Statement, TableFactor, TableObject, TableWithJoins, Value,
};
use sqlparser::dialect::{GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

/// Mapping from canonical SQL type names to Python type strings.
static SQL_TO_PYTHON_TYPE: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    HashMap::from([
        ("INT", "int"),
        ("INTEGER", "int"),
        ("BIGINT", "int"),
        ("SMALLINT", "int"),
        ("TINYINT", "int"),
        ("SIGNED", "int"),
        ("UNSIGNED", "int"),
        ("UBIGINT", "int"),
        ("FLOAT", "float"),
        ("DOUBLE", "float"),
        ("REAL", "float"),
        ("DECIMAL", "float"),
        ("NUMERIC", "float"),
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
    ])
});

/// Result types of functions by name (COUNT, AVG, SUM, MIN, MAX and COALESCE are special-cased).
static FUNCTION_TYPES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let groups: [(&[&str], &str); 4] = [
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
                "NOW",
                "CURRENT_TIMESTAMP",
                "CURRENT_DATE",
                "CURRENT_TIME",
            ],
            "str",
        ),
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

/// Schema: table name -> columns as `(name, canonical SQL type)` in declaration order.
pub type Schema = HashMap<String, Vec<(String, String)>>;

/// `(reference, table)` pairs for the tables a statement reads, in query order.
type Scope = Vec<(String, String)>;

/// A column's Python type, `None` when unknown (rendered as `str`).
type Ty = Option<String>;

/// Inference fails outright only for `SUM(column)` of a column missing from a schema table.
type Inferred<T> = Result<T, String>;

fn make_dialect(dialect: &str) -> Box<dyn sqlparser::dialect::Dialect> {
    match dialect.to_lowercase().as_str() {
        "postgres" | "postgresql" => Box::new(PostgreSqlDialect {}),
        "mysql" => Box::new(MySqlDialect {}),
        "sqlite" => Box::new(SQLiteDialect {}),
        _ => Box::new(GenericDialect {}),
    }
}

fn parse_first(sql: &str, dialect: &str) -> Option<Statement> {
    Parser::parse_sql(make_dialect(dialect).as_ref(), sql)
        .ok()?
        .into_iter()
        .next()
}

/// Spell a SQL type the way sqlglot does, e.g. `INTEGER` -> `INT`, `INT[]` -> `ARRAY<INT>`.
fn canonical_type(dt: &DataType, dialect: &str) -> String {
    if let DataType::Array(elem) = dt {
        return match elem {
            ArrayElemTypeDef::None => "ARRAY".to_string(),
            ArrayElemTypeDef::AngleBracket(inner)
            | ArrayElemTypeDef::SquareBracket(inner, _)
            | ArrayElemTypeDef::Parenthesis(inner)
            | ArrayElemTypeDef::Qualified(inner, _) => {
                format!("ARRAY<{}>", canonical_type(inner, dialect))
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
    let postgres = matches!(dialect, "postgres" | "postgresql");
    let base = match name.as_str() {
        "TIMESTAMP WITH TIME ZONE" => "TIMESTAMPTZ",
        "TIME WITH TIME ZONE" => "TIMETZ",
        "TIMESTAMP WITHOUT TIME ZONE" => "TIMESTAMP",
        "TIME WITHOUT TIME ZONE" => "TIME",
        "INTEGER" | "INT4" => "INT",
        "INT2" => "SMALLINT",
        "INT8" => "BIGINT",
        "FLOAT" if postgres => "DOUBLE",
        "FLOAT8" | "DOUBLE PRECISION" => "DOUBLE",
        "FLOAT4" | "REAL" => "FLOAT",
        "NUMERIC" => "DECIMAL",
        "CHARACTER VARYING" => "VARCHAR",
        "CHARACTER" => "CHAR",
        "STRING" => "TEXT",
        "BOOL" => "BOOLEAN",
        "BYTEA" | "BLOB" => "VARBINARY",
        other => other,
    };
    let base = match (dialect, base) {
        ("mysql", "TIMESTAMP") => "TIMESTAMPTZ",
        ("mysql", "SIGNED") => "BIGINT",
        ("mysql", "UNSIGNED") => "UBIGINT",
        _ => base,
    };
    format!("{base}{args}")
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

/// Load schema from CREATE TABLE SQL statements.
pub fn load_schema(schema_sql: &str, dialect: &str) -> Schema {
    let dialect = dialect.to_lowercase();
    schema_sql
        .split(';')
        .filter_map(|stmt| parse_first(stmt.trim(), &dialect))
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
                    let sql_type = canonical_type(&col.data_type, &dialect);
                    (col.name.value.to_lowercase(), sql_type)
                })
                .collect();
            (object_name(&create.name), columns)
        })
        .collect()
}

/// Infer the Python return type for a SQL query, e.g. `tuple[int, str]`.
pub fn infer_type(sql: &str, dialect: &str, schema: &Option<Schema>) -> Inferred<Option<String>> {
    let empty = Schema::new();
    let analyzer = Analyzer {
        schema: schema.as_ref().unwrap_or(&empty),
        dialect: dialect.to_lowercase(),
    };
    let Some(stmt) = parse_first(sql, dialect) else {
        return Ok(None);
    };
    let types = analyzer.statement_columns(&stmt)?;
    Ok(types.map(|types| {
        let types: Vec<&str> = types
            .iter()
            .map(|ty| ty.as_deref().unwrap_or("str"))
            .collect();
        format!("tuple[{}]", types.join(", "))
    }))
}

/// Whether `sql` yields rows: a query, DML with RETURNING, or unparseable SQL.
///
/// Empty SQL, DDL, DML without RETURNING and transaction control return no rows.
pub fn returns_rows(sql: &str, dialect: &str) -> bool {
    match Parser::parse_sql(make_dialect(dialect).as_ref(), sql) {
        Err(_) => true,
        Ok(stmts) => stmts.first().is_some_and(statement_returns_rows),
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

/// The non-empty top-level RETURNING list of a DML statement.
fn returning(stmt: &Statement) -> Option<&[SelectItem]> {
    let items = match stmt {
        Statement::Insert(insert) => &insert.returning,
        Statement::Update(update) => &update.returning,
        Statement::Delete(delete) => &delete.returning,
        Statement::Merge(merge) => match &merge.output {
            Some(OutputClause::Returning { select_items, .. }) => return Some(select_items),
            _ => return None,
        },
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

/// Scope of a SELECT: its FROM tables, each followed by its joins.
fn select_scope(from: &[TableWithJoins]) -> Scope {
    from.iter()
        .flat_map(|table| {
            std::iter::once(&table.relation).chain(table.joins.iter().map(|join| &join.relation))
        })
        .flat_map(table_scope)
        .collect()
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

struct Analyzer<'a> {
    schema: &'a Schema,
    dialect: String,
}

impl Analyzer<'_> {
    fn statement_columns(&self, stmt: &Statement) -> Inferred<Option<Vec<Ty>>> {
        match stmt {
            Statement::Query(query) => self.query_columns(query),
            _ => match returning(stmt) {
                Some(items) => self.projection(items, &dml_scope(stmt)).map(non_empty),
                None => Ok(None),
            },
        }
    }

    fn query_columns(&self, query: &Query) -> Inferred<Option<Vec<Ty>>> {
        match query.body.as_ref() {
            SetExpr::Insert(stmt)
            | SetExpr::Update(stmt)
            | SetExpr::Delete(stmt)
            | SetExpr::Merge(stmt) => self.statement_columns(stmt),
            body => match leftmost_select(body) {
                Some(select) => self
                    .projection(&select.projection, &select_scope(&select.from))
                    .map(non_empty),
                None => Ok(None),
            },
        }
    }

    /// One type per result column; a star may yield several.
    fn projection(&self, items: &[SelectItem], scope: &Scope) -> Inferred<Vec<Ty>> {
        let mut types = Vec::new();
        for item in items {
            let star = match item {
                SelectItem::UnnamedExpr(expr)
                | SelectItem::ExprWithAlias { expr, .. }
                | SelectItem::ExprWithAliases { expr, .. } => {
                    types.push(self.expr(expr, scope)?);
                    continue;
                }
                SelectItem::Wildcard(_) => self.star_columns(None, scope),
                SelectItem::QualifiedWildcard(
                    SelectItemQualifiedWildcardKind::ObjectName(name),
                    _,
                ) => self.star_columns(Some(&object_name(name)), scope),
                SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::Expr(_), _) => None,
            };
            match star {
                Some(columns) => types.extend(columns.iter().map(|(_, ty)| Some(python_type(ty)))),
                None => types.push(None),
            }
        }
        Ok(types)
    }

    /// Expand `*` (all scope tables) or `t.*` from the schema, or `None` if unknown.
    fn star_columns(
        &self,
        reference: Option<&str>,
        scope: &Scope,
    ) -> Option<Vec<(String, String)>> {
        let tables: Vec<String> = match reference {
            Some(reference) => vec![resolve(reference, scope)],
            None => scope.iter().map(|(_, table)| table.clone()).collect(),
        };
        if tables.is_empty() {
            return None;
        }
        let columns = tables
            .iter()
            .map(|table| self.schema.get(table))
            .collect::<Option<Vec<_>>>()?;
        Some(columns.into_iter().flatten().cloned().collect())
    }

    /// Schema type of a column: its qualifier's table, else the first scope table having it.
    fn column_type(&self, expr: &Expr, scope: &Scope) -> Option<String> {
        let lookup = |table: &str, column: &str| {
            self.schema
                .get(table)?
                .iter()
                .find(|(name, _)| name == column)
                .map(|(_, ty)| ty.clone())
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

    fn expr(&self, expr: &Expr, scope: &Scope) -> Inferred<Ty> {
        let known = |ty: &str| Ok(Some(ty.to_string()));
        match expr {
            Expr::Cast { data_type, .. } => {
                Ok(Some(python_type(&canonical_type(data_type, &self.dialect))))
            }
            Expr::Nested(inner) => self.expr(inner, scope),
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => {
                Ok(self.column_type(expr, scope).map(|ty| python_type(&ty)))
            }
            Expr::Value(value) => match &value.value {
                Value::Number(number, _) if number.contains(['.', 'e', 'E']) => known("float"),
                Value::Number(..) => known("int"),
                Value::Boolean(_) => known("bool"),
                Value::Null | Value::Placeholder(_) => Ok(None),
                _ => known("str"),
            },
            Expr::Exists { .. } | Expr::InSubquery { .. } => known("bool"),
            Expr::Subquery(query) => match leftmost_select(&query.body) {
                Some(select) if !select.projection.is_empty() => {
                    let scope = select_scope(&select.from);
                    Ok(self
                        .projection(&select.projection[..1], &scope)?
                        .into_iter()
                        .next()
                        .flatten())
                }
                _ => Ok(None),
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
                    None => Ok(None),
                }
            }
            Expr::Function(func) => {
                let name = func.name.to_string().to_uppercase();
                let arg = first_arg(&func.args);
                match name.as_str() {
                    "COUNT" => known("int"),
                    "AVG" => known("float"),
                    "SUM" | "MIN" | "MAX" => self.aggregate(&name, arg, scope),
                    "COALESCE" => arg.map_or(Ok(None), |arg| self.expr(arg, scope)),
                    _ => Ok(FUNCTION_TYPES.get(name.as_str()).map(|ty| ty.to_string())),
                }
            }
            _ => Ok(None),
        }
    }

    /// SUM/MIN/MAX take their argument's type; SUM falls back to float.
    fn aggregate(&self, name: &str, arg: Option<&Expr>, scope: &Scope) -> Inferred<Ty> {
        let ty = arg.map_or(Ok(None), |arg| self.expr(arg, scope))?;
        if ty.is_some() || name != "SUM" || scope.is_empty() {
            return Ok(ty.or_else(|| (name == "SUM").then(|| "float".to_string())));
        }
        let (table, column) = match arg {
            Some(Expr::Identifier(ident)) => (scope[0].1.clone(), &ident.value),
            Some(Expr::CompoundIdentifier(parts)) if parts.len() >= 2 => (
                resolve(&parts[parts.len() - 2].value, scope),
                &parts[parts.len() - 1].value,
            ),
            _ => return Ok(Some("float".to_string())),
        };
        if self.schema.contains_key(&table) {
            return Err(format!(
                "Column '{column}' not found in table '{table}' in schema. \
                 If you want to query columns not in the schema, use an explicit cast: \
                 SUM({column}::TYPE)"
            ));
        }
        Ok(Some("float".to_string()))
    }
}

fn non_empty(types: Vec<Ty>) -> Option<Vec<Ty>> {
    (!types.is_empty()).then_some(types)
}

/// Expand SELECT * into an explicit `col::TYPE` list, keeping other projections verbatim.
pub fn expand_star(sql: &str, dialect: &str, schema: &Option<Schema>) -> Option<String> {
    let schema = schema.as_ref().filter(|schema| !schema.is_empty())?;
    let analyzer = Analyzer {
        schema,
        dialect: dialect.to_lowercase(),
    };
    let Statement::Query(query) = parse_first(sql, dialect)? else {
        return None;
    };
    let scope = select_scope(&leftmost_select(&query.body)?.from);

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
        let columns = analyzer.star_columns(star.get(1).map(|m| m.as_str()), &scope)?;
        items.extend(columns.iter().map(|(name, ty)| format!("{name}::{ty}")));
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
    fn test_sum_of_unknown_schema_column_is_an_error() {
        let schema = Some(load_schema("CREATE TABLE users (id INT)", "postgres"));
        assert!(infer_type("SELECT SUM(nope) FROM users", "postgres", &schema).is_err());
    }
}
