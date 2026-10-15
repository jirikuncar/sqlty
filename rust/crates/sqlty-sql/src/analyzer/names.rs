//! Output-column names, following PostgreSQL's naming of unaliased expressions.

use std::borrow::Cow;

use sqlparser::ast::TrimWhereField;
use sqlparser::ast::{AccessExpr, DataType, Expr, Ident, ObjectName, Query, SelectItem};

use crate::catalog::fold;

use super::dialect::SqlDialect;
use super::query::leftmost_select;

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

/// A compound access `t.col[1].field`: sqlparser reads `t.col` in it as an identifier followed
/// by a dot access. Returns the column reference `t.col` and the accesses that follow it, or
/// `root` and the whole chain when the access does not start that way.
pub(super) fn split_access<'e>(
    root: &'e Expr,
    chain: &'e [AccessExpr],
) -> (Cow<'e, Expr>, &'e [AccessExpr]) {
    match (root, chain) {
        (Expr::Identifier(table), [AccessExpr::Dot(Expr::Identifier(column)), rest @ ..]) => (
            Cow::Owned(Expr::CompoundIdentifier(vec![
                table.clone(),
                column.clone(),
            ])),
            rest,
        ),
        _ => (Cow::Borrowed(root), chain),
    }
}

#[cfg(test)]
mod tests {
    use sqlparser::ast::{DataType, Ident, ObjectName};

    use super::super::test_support::expr;
    use super::*;

    fn object(name: &str) -> ObjectName {
        match expr(&format!("{name}()")) {
            Expr::Function(func) => func.name,
            other => panic!("not a call: {other:?}"),
        }
    }

    #[test]
    fn column_names_follow_postgres_figure_colname() {
        let exprs = [
            "x",
            "t.x",
            "\"Mixed\"",
            "x::int",
            "'a'::text",
            "'a'::text::int",
            "1::int",
            "1::bigint",
            "1::double precision",
            "1::varchar(10)",
            "1::timestamp with time zone",
            "count(*)",
            "pg_catalog.lower(x)",
            "case when x then 1 end",
            "case when x then 1 else y end",
            "exists (select 1)",
            "array[1, 2]",
            "row(1, 2)",
            "(select x from t)",
            "(select x as y from t)",
            "(t.col).field",
            "extract(year from x)",
            "trim(leading 'a' from x)",
            "trim(x)",
            "substring(x from 1)",
            "interval '1 day'",
            "x + 1",
            "-x",
            "1",
            "'literal'",
        ];
        let rendered: Vec<String> = exprs
            .iter()
            .map(|sql| format!("{sql} => {}", column_name(&expr(sql))))
            .collect();
        insta::assert_snapshot!(rendered.join("\n"));
    }

    #[test]
    fn figure_name_is_empty_where_column_name_falls_back() {
        assert_eq!(figure_name(&expr("x + 1")), "");
        assert_eq!(column_name(&expr("x + 1")), "?column?");
        assert_eq!(figure_name(&expr("x")), "x");
    }

    #[test]
    fn object_name_is_the_lowercased_last_part() {
        assert_eq!(object_name(&object("Public.Users")), "users");
        assert_eq!(object_name(&object("\"Quoted\"")), "quoted");
        assert_eq!(object_name(&ObjectName(Vec::new())), "");
    }

    #[test]
    fn table_key_keeps_at_most_schema_and_name() {
        let key = |name: &str, dialect| table_key(&object(name), dialect);
        assert_eq!(key("users", SqlDialect::Postgres), "users");
        assert_eq!(key("public.users", SqlDialect::Postgres), "public.users");
        assert_eq!(key("db.public.users", SqlDialect::Postgres), "public.users");
    }

    #[test]
    fn table_key_folds_by_dialect() {
        let key = |name: &str, dialect| table_key(&object(name), dialect);
        assert_eq!(key("Users", SqlDialect::Postgres), "users");
        assert_eq!(key("\"Users\"", SqlDialect::Postgres), "Users");
        assert_eq!(key("\"Users\"", SqlDialect::MySql), "users");
        assert_eq!(key("Users", SqlDialect::Sqlite), "users");
    }

    #[test]
    fn identifiers_fold_in_postgres_unless_quoted() {
        assert_eq!(ident_name(&Ident::new("Id")), "id");
        assert_eq!(ident_name(&Ident::with_quote('"', "Id")), "Id");
    }

    #[test]
    fn aliases_keep_their_case_outside_postgres() {
        let alias = Ident::new("TotalCount");
        assert_eq!(alias_name(&alias, SqlDialect::Postgres), "totalcount");
        assert_eq!(alias_name(&alias, SqlDialect::MySql), "TotalCount");
        let quoted = Ident::with_quote('"', "TotalCount");
        assert_eq!(alias_name(&quoted, SqlDialect::Postgres), "TotalCount");
    }

    #[test]
    fn cast_target_names_are_postgres_type_names() {
        let name = |sql: &str| match expr(&format!("1::{sql}")) {
            Expr::Cast { data_type, .. } => type_column_name(&data_type),
            other => panic!("not a cast: {other:?}"),
        };
        assert_eq!(name("integer"), "int4");
        assert_eq!(name("smallint"), "int2");
        assert_eq!(name("boolean"), "bool");
        assert_eq!(name("real"), "float4");
        assert_eq!(name("float(10)"), "float4");
        assert_eq!(name("float(40)"), "float8");
        assert_eq!(name("decimal(10, 2)"), "numeric");
        assert_eq!(name("char(3)"), "bpchar");
        assert_eq!(name("timestamp"), "timestamp");
        assert_eq!(name("timestamptz"), "timestamptz");
        assert_eq!(name("time with time zone"), "timetz");
        assert_eq!(name("int[]"), "int4");
        assert_eq!(name("public.mood"), "mood");
        assert_eq!(type_column_name(&DataType::Text), "text");
    }

    #[test]
    fn split_access_joins_a_table_and_column_before_the_rest_of_the_chain() {
        let Expr::CompoundFieldAccess { root, access_chain } = expr("t.col.field[1]") else {
            panic!("expected a compound access");
        };
        let (column, rest) = split_access(&root, &access_chain);
        assert_eq!(column.to_string(), "t.col");
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn split_access_leaves_other_roots_alone() {
        let Expr::CompoundFieldAccess { root, access_chain } = expr("(x)[1]") else {
            panic!("expected a compound access");
        };
        let (column, rest) = split_access(&root, &access_chain);
        assert_eq!(column.to_string(), root.to_string());
        assert_eq!(rest.len(), access_chain.len());
    }
}
