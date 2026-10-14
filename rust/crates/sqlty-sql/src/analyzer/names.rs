//! Output-column names, following PostgreSQL's naming of unaliased expressions.

use super::*;

/// Lowercased unqualified name: `public.users` -> `users`.
pub(super) fn object_name(name: &ObjectName) -> String {
    name.0
        .last()
        .and_then(|part| part.as_ident())
        .map(|ident| ident.value.to_lowercase())
        .unwrap_or_default()
}

/// PostgreSQL's name for an unquoted identifier is lowercase.
pub(super) fn ident_name(ident: &Ident) -> String {
    match ident.quote_style {
        Some(_) => ident.value.clone(),
        None => ident.value.to_lowercase(),
    }
}

/// Name of an unaliased result column, as PostgreSQL picks it: a column's name (also under
/// a cast), a function's name, a few keywords (`case`, `exists`, `array`, ...), and
/// `?column?` for anything else.
pub(super) fn column_name(expr: &Expr) -> String {
    let name = figure_name(expr);
    if name.is_empty() {
        "?column?".to_string()
    } else {
        name
    }
}

/// [`column_name`], empty when PostgreSQL gives the column no name of its own.
pub(super) fn figure_name(expr: &Expr) -> String {
    match expr {
        Expr::Identifier(ident) => ident_name(ident),
        Expr::CompoundIdentifier(parts) => parts.last().map(ident_name).unwrap_or_default(),
        Expr::Cast {
            expr, data_type, ..
        } => Some(figure_name(expr))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| type_column_name(data_type)),
        Expr::Nested(expr) | Expr::Collate { expr, .. } => figure_name(expr),
        Expr::CompoundFieldAccess { root, access_chain } => access_chain
            .iter()
            .rev()
            .find_map(|access| match access {
                AccessExpr::Dot(Expr::Identifier(field)) => Some(ident_name(field)),
                _ => None,
            })
            .unwrap_or_else(|| figure_name(root)),
        Expr::Function(func) => func
            .name
            .0
            .last()
            .and_then(|part| part.as_ident())
            .map(ident_name)
            .unwrap_or_default(),
        Expr::Extract { .. } => "extract".to_string(),
        Expr::Case { .. } => "case".to_string(),
        Expr::Exists { .. } => "exists".to_string(),
        Expr::Array(_) => "array".to_string(),
        Expr::Trim { .. } => "btrim".to_string(),
        Expr::Substring { .. } => "substring".to_string(),
        Expr::Position { .. } => "position".to_string(),
        Expr::Overlay { .. } => "overlay".to_string(),
        Expr::AtTimeZone { .. } => "timezone".to_string(),
        Expr::Interval(_) => "interval".to_string(),
        Expr::TypedString(typed) => type_column_name(&typed.data_type),
        Expr::Subquery(query) => scalar_subquery_name(query),
        _ => String::new(),
    }
}

/// The name PostgreSQL gives `value::type` when `value` has none: the type's own name.
pub(super) fn type_column_name(data_type: &DataType) -> String {
    let text = data_type.to_string().to_lowercase();
    let base = text.split(['(', '[']).next().unwrap_or_default().trim();
    match base {
        "int" | "integer" => "int4",
        "bigint" => "int8",
        "smallint" => "int2",
        "boolean" => "bool",
        "real" => "float4",
        "double precision" | "double" => "float8",
        "character varying" => "varchar",
        "character" | "char" => "bpchar",
        "decimal" => "numeric",
        "float" => "float8",
        "timestamp" | "timestamp without time zone" => "timestamp",
        "timestamp with time zone" => "timestamptz",
        "time without time zone" => "time",
        "time with time zone" => "timetz",
        other => other.rsplit('.').next().unwrap_or(other),
    }
    .to_string()
}

/// A scalar subquery is named after its single column.
pub(super) fn scalar_subquery_name(query: &Query) -> String {
    match leftmost_select(&query.body).and_then(|select| select.projection.first()) {
        Some(SelectItem::UnnamedExpr(expr)) => figure_name(expr),
        Some(SelectItem::ExprWithAlias { alias, .. }) => alias.value.clone(),
        _ => String::new(),
    }
}
