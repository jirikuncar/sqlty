//! Output-column names, following PostgreSQL's naming of unaliased expressions.

use super::*;
use sqlparser::ast::TrimWhereField;

/// Lowercased unqualified name, which is how aliases and qualifiers are compared: `public.users` -> `users`.
pub(super) fn object_name(name: &ObjectName) -> String {
    name.0
        .last()
        .and_then(|part| part.as_ident())
        .map(|ident| ident.value.to_lowercase())
        .unwrap_or_default()
}

/// The key a table reference is looked up by: `schema.name` when qualified, else `name`,
/// each part folded as the dialect does (PostgreSQL keeps quoted case).
pub(super) fn table_key(name: &ObjectName, dialect: SqlDialect) -> String {
    let parts: Vec<String> = name
        .0
        .iter()
        .filter_map(|part| part.as_ident())
        .map(|ident| fold(ident, dialect))
        .collect();
    parts[parts.len().saturating_sub(2)..].join(".")
}

/// PostgreSQL's name for an unquoted identifier is lowercase.
pub(super) fn ident_name(ident: &Ident) -> String {
    match ident.quote_style {
        Some(_) => ident.value.clone(),
        None => ident.value.to_lowercase(),
    }
}

/// The name an alias gives an output column: PostgreSQL folds an unquoted one to lower case;
/// the other dialects keep it as written.
pub(super) fn alias_name(ident: &Ident, dialect: SqlDialect) -> String {
    if dialect == SqlDialect::Postgres {
        ident_name(ident)
    } else {
        ident.value.clone()
    }
}

/// Name of an unaliased result column, as PostgreSQL picks it: a column's name (also under
/// a cast), a function's name, a few keywords (`case`, `exists`, `array`, ...), the target
/// type of a cast of an unnamed value (`1::int` is `int4`), and `?column?` for anything
/// else.
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
    name_strength(expr).0
}

/// How firmly an expression names its column: an own name (a column, a function, a
/// keyword like `case`) is `Named`, a type name taken from a cast is `Weak` and a cast
/// replaces it, and no name at all is `None`. This is PostgreSQL's `FigureColname`:
/// `x::int` is `x`, `'a'::text` is `text` and `'a'::text::int` is `int4`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Strength {
    None,
    Weak,
    Named,
}

fn name_strength(expr: &Expr) -> (String, Strength) {
    let named = |name: &str| (name.to_string(), Strength::Named);
    let own = |name: String| {
        let strength = if name.is_empty() {
            Strength::None
        } else {
            Strength::Named
        };
        (name, strength)
    };
    match expr {
        Expr::Identifier(ident) => own(ident_name(ident)),
        Expr::CompoundIdentifier(parts) => own(parts.last().map(ident_name).unwrap_or_default()),
        Expr::Cast {
            expr, data_type, ..
        } => match name_strength(expr) {
            (name, Strength::Named) => (name, Strength::Named),
            _ => (type_column_name(data_type), Strength::Weak),
        },
        Expr::Nested(expr) | Expr::Collate { expr, .. } => name_strength(expr),
        Expr::CompoundFieldAccess { root, access_chain } => access_chain
            .iter()
            .rev()
            .find_map(|access| match access {
                AccessExpr::Dot(Expr::Identifier(field)) => Some(own(ident_name(field))),
                _ => None,
            })
            .unwrap_or_else(|| name_strength(root)),
        Expr::Function(func) => own(func
            .name
            .0
            .last()
            .and_then(|part| part.as_ident())
            .map(ident_name)
            .unwrap_or_default()),
        Expr::Extract { .. } => named("extract"),
        // The ELSE result's own name, else a weak `case` (`FigureColnameInternal`, T_CaseExpr).
        Expr::Case { else_result, .. } => match else_result.as_deref().map(name_strength) {
            Some((name, Strength::Named)) => (name, Strength::Named),
            _ => ("case".to_string(), Strength::Weak),
        },
        Expr::Exists { .. } => named("exists"),
        Expr::Array(_) => named("array"),
        Expr::Tuple(_) => named("row"),
        Expr::Ceil { .. } => named("ceil"),
        Expr::Floor { .. } => named("floor"),
        Expr::Trim { trim_where, .. } => named(match trim_where {
            Some(TrimWhereField::Leading) => "ltrim",
            Some(TrimWhereField::Trailing) => "rtrim",
            _ => "btrim",
        }),
        Expr::Substring { .. } => named("substring"),
        Expr::Position { .. } => named("position"),
        Expr::Overlay { .. } => named("overlay"),
        Expr::AtTimeZone { .. } => named("timezone"),
        Expr::Interval(_) => ("interval".to_string(), Strength::Weak),
        Expr::TypedString(typed) => (type_column_name(&typed.data_type), Strength::Weak),
        Expr::Subquery(query) => scalar_subquery_name(query),
        _ => (String::new(), Strength::None),
    }
}

/// The name PostgreSQL gives `value::type` when `value` has none: the type's own name.
pub(super) fn type_column_name(data_type: &DataType) -> String {
    let text = data_type.to_string().to_lowercase();
    let base = text.split(['(', '[']).next().unwrap_or_default().trim();
    let zoned = text.contains("with time zone");
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
        "float" => match text
            .split(['(', ')'])
            .nth(1)
            .and_then(|p| p.trim().parse().ok())
        {
            Some(1..=24) => "float4",
            _ => "float8",
        },
        "timestamp" | "timestamp without time zone" if zoned => "timestamptz",
        "timestamp" | "timestamp without time zone" => "timestamp",
        "timestamp with time zone" => "timestamptz",
        "time" | "time without time zone" if zoned => "timetz",
        "time without time zone" => "time",
        "time with time zone" => "timetz",
        other => other.rsplit('.').next().unwrap_or(other),
    }
    .to_string()
}

/// A scalar subquery is named after its single column.
fn scalar_subquery_name(query: &Query) -> (String, Strength) {
    match leftmost_select(&query.body).and_then(|select| select.projection.first()) {
        Some(SelectItem::UnnamedExpr(expr)) => name_strength(expr),
        Some(SelectItem::ExprWithAlias { alias, .. }) => (alias.value.clone(), Strength::Named),
        _ => (String::new(), Strength::None),
    }
}
