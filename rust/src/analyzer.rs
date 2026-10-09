//! SQL type inference engine — replaces sqlglot-based Python implementation.
//!
//! Maps SQL types to Python types and infers return types from SQL queries.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use sqlparser::ast::{
    self, DataType, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Query,
    Select, SelectItem, SetExpr, Statement, TableFactor, TableWithJoins, Value,
};
use sqlparser::dialect::{
    GenericDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect,
};
use sqlparser::parser::Parser;

/// Mapping from uppercase SQL type names to Python type strings.
static SQL_TO_PYTHON_TYPE: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("INT", "int");
    m.insert("INTEGER", "int");
    m.insert("BIGINT", "int");
    m.insert("SMALLINT", "int");
    m.insert("TINYINT", "int");
    m.insert("FLOAT", "float");
    m.insert("DOUBLE", "float");
    m.insert("REAL", "float");
    m.insert("DECIMAL", "float");
    m.insert("NUMERIC", "float");
    m.insert("TEXT", "str");
    m.insert("VARCHAR", "str");
    m.insert("CHAR", "str");
    m.insert("STRING", "str");
    m.insert("BOOLEAN", "bool");
    m.insert("BOOL", "bool");
    m.insert("DATE", "datetime.date");
    m.insert("TIMESTAMP", "datetime.datetime");
    m.insert("TIMESTAMPTZ", "datetime.datetime");
    m.insert("DATETIME", "datetime.datetime");
    m.insert("TIME", "datetime.time");
    m.insert("TIMETZ", "datetime.time");
    m.insert("INTERVAL", "datetime.timedelta");
    m.insert("ARRAY", "list");
    m.insert("JSON", "dict");
    m.insert("JSONB", "dict");
    m.insert("UUID", "uuid.UUID");
    // MySQL-specific
    m.insert("SIGNED", "int");
    m.insert("UNSIGNED", "int");
    m
});

/// Schema: table_name -> { column_name -> sql_type }
pub type Schema = HashMap<String, HashMap<String, String>>;

/// Parse a SQL dialect string into a boxed dialect for sqlparser.
fn make_dialect(dialect: &str) -> Box<dyn sqlparser::dialect::Dialect> {
    match dialect.to_lowercase().as_str() {
        "postgres" | "postgresql" => Box::new(PostgreSqlDialect {}),
        "mysql" => Box::new(MySqlDialect {}),
        "sqlite" => Box::new(SQLiteDialect {}),
        _ => Box::new(GenericDialect {}),
    }
}

/// Normalize a SQL type name for lookup in the type map.
fn normalize_sql_type(dt: &DataType) -> String {
    let s = dt.to_string().to_uppercase();
    let base = s.split('(').next().unwrap_or(&s).trim().to_string();
    if base.contains("WITH TIME ZONE") || base.contains("WITH_TIME_ZONE") {
        if base.starts_with("TIMESTAMP") {
            return "TIMESTAMPTZ".to_string();
        }
        if base.starts_with("TIME") {
            return "TIMETZ".to_string();
        }
    }
    base
}

/// Map a SQL DataType to a Python type string.
fn sql_type_to_python(dt: &DataType) -> String {
    let normalized = normalize_sql_type(dt);
    SQL_TO_PYTHON_TYPE
        .get(normalized.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "str".to_string())
}

/// Map a SQL type name string to a Python type.
fn sql_type_name_to_python(name: &str) -> String {
    let upper = name.to_uppercase();
    let base = upper.split('(').next().unwrap_or(&upper).trim();
    SQL_TO_PYTHON_TYPE
        .get(base)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "str".to_string())
}

/// Load schema from CREATE TABLE SQL statements.
pub fn load_schema(schema_sql: &str, dialect: &str) -> Schema {
    let mut schema = Schema::new();
    let d = make_dialect(dialect);

    for stmt_str in schema_sql.split(';') {
        let trimmed = stmt_str.trim();
        if trimmed.is_empty() {
            continue;
        }

        let stmts = match Parser::parse_sql(d.as_ref(), trimmed) {
            Ok(s) => s,
            Err(_) => continue,
        };

        for stmt in stmts {
            if let Statement::CreateTable(create_table) = stmt {
                let table_name = create_table.name.to_string().to_lowercase();
                let mut columns = HashMap::new();

                for col_def in &create_table.columns {
                    let col_name = col_def.name.value.to_lowercase();
                    let col_type = normalize_sql_type(&col_def.data_type);
                    columns.insert(col_name, col_type);
                }

                schema.insert(table_name, columns);
            }
        }
    }

    schema
}

/// Column info extracted from a SQL query.
struct ColumnInfo {
    python_type: String,
}

/// Infer the Python return type for a SQL query.
pub fn infer_type(sql: &str, dialect: &str, schema: &Option<Schema>) -> Option<String> {
    let columns = analyze_select(sql, dialect, schema)?;
    if columns.is_empty() {
        return None;
    }

    let types: Vec<&str> = columns.iter().map(|c| c.python_type.as_str()).collect();
    Some(format!("tuple[{}]", types.join(", ")))
}

/// Whether `sql` yields rows: a query, DML with RETURNING, or unparseable SQL.
///
/// Empty SQL, DDL, DML without RETURNING and transaction control return no rows.
pub fn returns_rows(sql: &str, dialect: &str) -> bool {
    let d = make_dialect(dialect);
    match Parser::parse_sql(d.as_ref(), sql) {
        Err(_) => true,
        Ok(stmts) => stmts.first().is_some_and(statement_returns_rows),
    }
}

fn statement_returns_rows(stmt: &Statement) -> bool {
    let has_returning = |items: &Option<Vec<SelectItem>>| items.as_ref().is_some_and(|r| !r.is_empty());
    match stmt {
        Statement::Query(query) => match query.body.as_ref() {
            SetExpr::Insert(inner) | SetExpr::Update(inner) => statement_returns_rows(inner),
            _ => true,
        },
        Statement::Insert(insert) => has_returning(&insert.returning),
        Statement::Update { returning, .. } => has_returning(returning),
        Statement::Delete(delete) => has_returning(&delete.returning),
        _ => false,
    }
}

/// Analyze a SQL query and return column type information.
fn analyze_select(sql: &str, dialect: &str, schema: &Option<Schema>) -> Option<Vec<ColumnInfo>> {
    let d = make_dialect(dialect);
    let stmts = Parser::parse_sql(d.as_ref(), sql).ok()?;
    let stmt = stmts.into_iter().next()?;

    match stmt {
        Statement::Query(query) => process_query(&query, schema),
        Statement::Insert(insert) => {
            if let Some(ref returning) = insert.returning {
                if !returning.is_empty() {
                    let table_name = insert.table_name.to_string().to_lowercase();
                    return Some(process_select_items(returning, schema, Some(&table_name)));
                }
            }
            None
        }
        Statement::Update { returning, .. } => {
            if let Some(ref returning_items) = returning {
                if !returning_items.is_empty() {
                    return Some(process_select_items(returning_items, schema, None));
                }
            }
            None
        }
        Statement::Delete(delete) => {
            if let Some(ref returning_items) = delete.returning {
                if !returning_items.is_empty() {
                    return Some(process_select_items(returning_items, schema, None));
                }
            }
            None
        }
        _ => None,
    }
}

/// Process a top-level Query (SELECT / WITH).
fn process_query(query: &Query, schema: &Option<Schema>) -> Option<Vec<ColumnInfo>> {
    match query.body.as_ref() {
        SetExpr::Select(select) => process_select(select, schema),
        SetExpr::SetOperation { left, .. } => {
            if let SetExpr::Select(select) = left.as_ref() {
                process_select(select, schema)
            } else {
                None
            }
        }
        SetExpr::Query(inner_query) => process_query(inner_query, schema),
        _ => None,
    }
}

/// Process a SELECT statement.
fn process_select(select: &Select, schema: &Option<Schema>) -> Option<Vec<ColumnInfo>> {
    if select.projection.is_empty() {
        return None;
    }

    let table_map = build_table_map(&select.from);
    let mut columns = Vec::new();

    for item in &select.projection {
        match item {
            SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                let col = infer_expr_type(expr, schema, &table_map);
                columns.push(col);
            }
            SelectItem::Wildcard(_) => {
                if let Some(ref schema_map) = schema {
                    for table_with_joins in &select.from {
                        let tables = extract_tables_from_join(table_with_joins);
                        for tname in tables {
                            let lower = tname.to_lowercase();
                            if let Some(cols) = schema_map.get(&lower) {
                                let mut col_names: Vec<&String> = cols.keys().collect();
                                col_names.sort();
                                for cn in col_names {
                                    let sql_type = &cols[cn];
                                    columns.push(ColumnInfo {
                                        python_type: sql_type_name_to_python(sql_type),
                                    });
                                }
                            }
                        }
                    }
                } else {
                    columns.push(ColumnInfo {
                        python_type: "str".to_string(),
                    });
                }
            }
            SelectItem::QualifiedWildcard(name, _) => {
                let table_ref = name.to_string().to_lowercase();
                if let Some(ref schema_map) = schema {
                    let real_table = table_map
                        .get(&table_ref)
                        .map(|s| s.as_str())
                        .unwrap_or(&table_ref);
                    if let Some(cols) = schema_map.get(real_table) {
                        let mut col_names: Vec<&String> = cols.keys().collect();
                        col_names.sort();
                        for cn in col_names {
                            let sql_type = &cols[cn];
                            columns.push(ColumnInfo {
                                python_type: sql_type_name_to_python(sql_type),
                            });
                        }
                    }
                }
            }
        }
    }

    if columns.is_empty() {
        None
    } else {
        Some(columns)
    }
}

/// Extract function arguments from FunctionArguments enum.
fn get_func_args(args: &FunctionArguments) -> Option<&[FunctionArg]> {
    match args {
        FunctionArguments::List(list) => Some(&list.args),
        _ => None,
    }
}

/// Infer the Python type for a single SQL expression.
fn infer_expr_type(
    expr: &Expr,
    schema: &Option<Schema>,
    table_map: &HashMap<String, String>,
) -> ColumnInfo {
    match expr {
        // CAST(x AS type) or x::type — covers Cast, TryCast, SafeCast via CastKind
        Expr::Cast { data_type, .. } => ColumnInfo {
            python_type: sql_type_to_python(data_type),
        },

        // Function calls
        Expr::Function(func) => infer_function_type(func, schema, table_map),

        // Column reference: table.column
        Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
            let table_ref = parts[parts.len() - 2].value.to_lowercase();
            let col_name = parts[parts.len() - 1].value.to_lowercase();
            if let Some(ref schema_map) = schema {
                let real_table = table_map
                    .get(&table_ref)
                    .map(|s| s.as_str())
                    .unwrap_or(&table_ref);
                if let Some(cols) = schema_map.get(real_table) {
                    if let Some(sql_type) = cols.get(&col_name) {
                        return ColumnInfo {
                            python_type: sql_type_name_to_python(sql_type),
                        };
                    }
                }
            }
            ColumnInfo {
                python_type: "str".to_string(),
            }
        }

        // Unqualified column
        Expr::Identifier(ident) => {
            let col_name = ident.value.to_lowercase();
            if let Some(ref schema_map) = schema {
                for (_, cols) in schema_map {
                    if let Some(sql_type) = cols.get(&col_name) {
                        return ColumnInfo {
                            python_type: sql_type_name_to_python(sql_type),
                        };
                    }
                }
            }
            ColumnInfo {
                python_type: "str".to_string(),
            }
        }

        // Nested expression (parenthesized)
        Expr::Nested(inner) => infer_expr_type(inner, schema, table_map),

        // Subquery
        Expr::Subquery(query) => {
            if let Some(cols) = process_query(query, schema) {
                if let Some(first) = cols.first() {
                    return ColumnInfo {
                        python_type: first.python_type.clone(),
                    };
                }
            }
            ColumnInfo {
                python_type: "str".to_string(),
            }
        }

        // CASE expression
        Expr::Case {
            results, else_result, ..
        } => {
            if let Some(first_result) = results.first() {
                return infer_expr_type(first_result, schema, table_map);
            }
            if let Some(else_expr) = else_result {
                return infer_expr_type(else_expr, schema, table_map);
            }
            ColumnInfo {
                python_type: "str".to_string(),
            }
        }

        // Exists subquery
        Expr::Exists { .. } => ColumnInfo {
            python_type: "bool".to_string(),
        },

        // InSubquery
        Expr::InSubquery { .. } => ColumnInfo {
            python_type: "bool".to_string(),
        },

        // Literal values
        Expr::Value(v) => match v {
            Value::Number(_, _) => ColumnInfo {
                python_type: "int".to_string(),
            },
            Value::SingleQuotedString(_) | Value::DoubleQuotedString(_) => ColumnInfo {
                python_type: "str".to_string(),
            },
            Value::Boolean(_) => ColumnInfo {
                python_type: "bool".to_string(),
            },
            Value::Null => ColumnInfo {
                python_type: "str".to_string(),
            },
            _ => ColumnInfo {
                python_type: "str".to_string(),
            },
        },

        // Default
        _ => ColumnInfo {
            python_type: "str".to_string(),
        },
    }
}

/// Infer type from a SQL function call.
fn infer_function_type(
    func: &ast::Function,
    schema: &Option<Schema>,
    table_map: &HashMap<String, String>,
) -> ColumnInfo {
    let func_name = func.name.to_string().to_uppercase();

    // Check if the function has a cast on its argument
    if let Some(args) = get_func_args(&func.args) {
        if let Some(FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Cast { data_type, .. }))) =
            args.first()
        {
            if matches!(func_name.as_str(), "SUM" | "AVG" | "MIN" | "MAX") {
                return ColumnInfo {
                    python_type: sql_type_to_python(data_type),
                };
            }
        }
    }

    // Check if SUM/AVG has a column reference and we have schema
    if matches!(func_name.as_str(), "SUM" | "AVG" | "MIN" | "MAX") {
        if let Some(col_type) = infer_aggregate_from_schema(func, schema, table_map) {
            return col_type;
        }
    }

    match func_name.as_str() {
        "COUNT" | "LENGTH" | "POSITION" | "CHAR_LENGTH" | "BIT_LENGTH" | "OCTET_LENGTH" => {
            ColumnInfo {
                python_type: "int".to_string(),
            }
        }
        "AVG" | "SUM" | "STDDEV" | "STDDEV_POP" | "STDDEV_SAMP" | "VARIANCE" | "VAR_POP"
        | "VAR_SAMP" => ColumnInfo {
            python_type: "float".to_string(),
        },
        "UPPER" | "LOWER" | "TRIM" | "CONCAT" | "SUBSTR" | "SUBSTRING" | "REPLACE"
        | "STRING_AGG" => ColumnInfo {
            python_type: "str".to_string(),
        },
        "COALESCE" => {
            if let Some(args) = get_func_args(&func.args) {
                if let Some(FunctionArg::Unnamed(FunctionArgExpr::Expr(expr))) = args.first() {
                    return infer_expr_type(expr, schema, table_map);
                }
            }
            ColumnInfo {
                python_type: "str".to_string(),
            }
        }
        "ROW_NUMBER" | "RANK" | "DENSE_RANK" | "NTILE" => ColumnInfo {
            python_type: "int".to_string(),
        },
        "NOW" | "CURRENT_TIMESTAMP" | "CURRENT_DATE" | "CURRENT_TIME" => ColumnInfo {
            python_type: "str".to_string(),
        },
        "JSON_BUILD_OBJECT" | "JSONB_BUILD_OBJECT" | "TO_JSON" | "TO_JSONB" | "JSON_AGG"
        | "JSONB_AGG" | "JSONB_OBJECT_AGG" | "JSON_OBJECT_AGG" => ColumnInfo {
            python_type: "dict".to_string(),
        },
        _ => ColumnInfo {
            python_type: "str".to_string(),
        },
    }
}

/// Try to infer aggregate function type from schema.
fn infer_aggregate_from_schema(
    func: &ast::Function,
    schema: &Option<Schema>,
    table_map: &HashMap<String, String>,
) -> Option<ColumnInfo> {
    let schema_map = schema.as_ref()?;
    let args = get_func_args(&func.args)?;
    let first_arg = args.first()?;

    match first_arg {
        FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::CompoundIdentifier(parts)))
            if parts.len() >= 2 =>
        {
            let table_ref = parts[parts.len() - 2].value.to_lowercase();
            let col_name = parts[parts.len() - 1].value.to_lowercase();
            let real_table = table_map
                .get(&table_ref)
                .map(|s| s.as_str())
                .unwrap_or(&table_ref);
            let cols = schema_map.get(real_table)?;
            let sql_type = cols.get(&col_name)?;
            Some(ColumnInfo {
                python_type: sql_type_name_to_python(sql_type),
            })
        }
        FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Identifier(ident))) => {
            let col_name = ident.value.to_lowercase();
            for (_, cols) in schema_map {
                if let Some(sql_type) = cols.get(&col_name) {
                    return Some(ColumnInfo {
                        python_type: sql_type_name_to_python(sql_type),
                    });
                }
            }
            None
        }
        _ => None,
    }
}

/// Process RETURNING clause items.
fn process_select_items(
    items: &[SelectItem],
    schema: &Option<Schema>,
    table_name: Option<&str>,
) -> Vec<ColumnInfo> {
    let table_map = HashMap::new();
    let mut columns = Vec::new();

    for item in items {
        match item {
            SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                let mut col = infer_expr_type(expr, schema, &table_map);
                // For unqualified columns in RETURNING, try table schema
                if col.python_type == "str" {
                    if let Expr::Identifier(ident) = expr {
                        if let Some(ref schema_map) = schema {
                            if let Some(tname) = table_name {
                                if let Some(cols) = schema_map.get(tname) {
                                    if let Some(sql_type) = cols.get(&ident.value.to_lowercase()) {
                                        col.python_type = sql_type_name_to_python(sql_type);
                                    }
                                }
                            }
                        }
                    }
                }
                columns.push(col);
            }
            SelectItem::Wildcard(_) => {
                if let Some(ref schema_map) = schema {
                    if let Some(tname) = table_name {
                        if let Some(cols) = schema_map.get(tname) {
                            let mut col_names: Vec<&String> = cols.keys().collect();
                            col_names.sort();
                            for cn in col_names {
                                let sql_type = &cols[cn];
                                columns.push(ColumnInfo {
                                    python_type: sql_type_name_to_python(sql_type),
                                });
                            }
                        }
                    }
                }
            }
            _ => {
                columns.push(ColumnInfo {
                    python_type: "str".to_string(),
                });
            }
        }
    }

    columns
}

/// Build a map from table alias/name -> real table name.
fn build_table_map(from: &[TableWithJoins]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for table_with_joins in from {
        add_table_to_map(&table_with_joins.relation, &mut map);
        for join in &table_with_joins.joins {
            add_table_to_map(&join.relation, &mut map);
        }
    }
    map
}

fn add_table_to_map(factor: &TableFactor, map: &mut HashMap<String, String>) {
    if let TableFactor::Table { name, alias, .. } = factor {
        let real_name = name.to_string().to_lowercase();
        map.insert(real_name.clone(), real_name.clone());
        if let Some(alias) = alias {
            map.insert(alias.name.value.to_lowercase(), real_name);
        }
    }
}

fn extract_tables_from_join(table_with_joins: &TableWithJoins) -> Vec<String> {
    let mut tables = Vec::new();
    if let TableFactor::Table { name, .. } = &table_with_joins.relation {
        tables.push(name.to_string().to_lowercase());
    }
    for join in &table_with_joins.joins {
        if let TableFactor::Table { name, .. } = &join.relation {
            tables.push(name.to_string().to_lowercase());
        }
    }
    tables
}

/// Expand SELECT * into explicit column list.
pub fn expand_star(sql: &str, dialect: &str, schema: &Option<Schema>) -> Option<String> {
    let schema_map = schema.as_ref()?;
    let d = make_dialect(dialect);
    let stmts = Parser::parse_sql(d.as_ref(), sql).ok()?;
    let stmt = stmts.into_iter().next()?;

    let query = match stmt {
        Statement::Query(q) => q,
        _ => return None,
    };

    let select = match query.body.as_ref() {
        SetExpr::Select(s) => s,
        _ => return None,
    };

    let has_star = select.projection.iter().any(|item| {
        matches!(
            item,
            SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)
        )
    });

    if !has_star {
        return None;
    }

    let table_map = build_table_map(&select.from);

    let mut expanded_cols = Vec::new();
    for item in &select.projection {
        match item {
            SelectItem::Wildcard(_) => {
                for table_with_joins in &select.from {
                    let tables = extract_tables_from_join(table_with_joins);
                    for tname in tables {
                        let cols = schema_map.get(&tname)?;
                        let mut col_names: Vec<&String> = cols.keys().collect();
                        col_names.sort();
                        for cn in col_names {
                            let sql_type = &cols[cn];
                            expanded_cols.push(format!("{}::{}", cn, sql_type));
                        }
                    }
                }
            }
            SelectItem::QualifiedWildcard(name, _) => {
                let table_ref = name.to_string().to_lowercase();
                let real_table = table_map
                    .get(&table_ref)
                    .map(|s| s.as_str())
                    .unwrap_or(&table_ref);
                let cols = schema_map.get(real_table)?;
                let mut col_names: Vec<&String> = cols.keys().collect();
                col_names.sort();
                for cn in col_names {
                    let sql_type = &cols[cn];
                    expanded_cols.push(format!("{}::{}", cn, sql_type));
                }
            }
            _ => {
                expanded_cols.push(item.to_string());
            }
        }
    }

    let sql_upper = sql.to_uppercase();
    let select_pos = sql_upper.find("SELECT")?;
    let from_pos = find_top_level_from(&sql_upper)?;

    let prefix = &sql[..select_pos];
    let suffix = &sql[from_pos..];
    let columns_str = expanded_cols.join(", ");

    Some(format!("{}SELECT {} {}", prefix, columns_str, suffix))
}

/// Find the position of the top-level FROM keyword.
fn find_top_level_from(sql_upper: &str) -> Option<usize> {
    let mut depth = 0;
    let bytes = sql_upper.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'F' if depth == 0 && i + 4 <= bytes.len() => {
                if &sql_upper[i..i + 4] == "FROM" {
                    let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
                    let after_ok =
                        i + 4 >= bytes.len() || !bytes[i + 4].is_ascii_alphanumeric();
                    if before_ok && after_ok {
                        return Some(i);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}
