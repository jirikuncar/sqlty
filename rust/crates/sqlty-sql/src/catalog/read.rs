//! Reading schema statements into loader operations, and the [`load`] entry point.

use std::rc::Rc;
use std::sync::LazyLock;

use sqlparser::ast::{Statement, UserDefinedTypeRepresentation};
use sqlparser::parser::Parser;

use crate::analyzer::{parse_ddl, SkippedObject, SqlDialect};
use crate::ddl;
use crate::values::{self};

use super::alter::changes_columns;
use super::columns::add_values;
use super::loader::{Loader, Op, OpKind, Taint, TypeDef};
use super::names::fold;
use super::Catalog;

pub(super) static COLLATE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(?i)\s+COLLATE\s+(?:"[^"]*"|[\w.]+)"#).unwrap());

pub(super) fn read_statements(
    statements: &[ddl::Statement],
    dialect: SqlDialect,
    loader: &mut Loader,
    skipped: &mut Vec<SkippedObject>,
) -> Vec<Op> {
    let postgres = dialect == SqlDialect::Postgres;
    let mut path = Rc::new(vec!["public".to_string()]);
    let mut ops = Vec::new();
    let parser_dialect = dialect.parser_dialect();
    let parse_part = |text: &str| Parser::new(parser_dialect.as_ref()).try_with_sql(text).ok();
    for stmt in statements {
        if postgres {
            if let Some(schemas) = stmt.search_path() {
                loader.search_path = schemas.clone();
                path = Rc::new(schemas);
                continue;
            }
        }
        let mut push = |kind: OpKind| {
            ops.push(Op {
                path: path.clone(),
                kind,
            });
        };
        if stmt.is_create_table() || (postgres && stmt.is_create_table_any()) {
            let (likes, text) = match postgres.then(|| stmt.without_like()).flatten() {
                Some((text, likes)) => (likes, Some(text)),
                None => (Vec::new(), None),
            };
            let parsed = match &text {
                None => parse_ddl(&stmt.sql(), dialect)
                    .or_else(|| parse_ddl(&stmt.without_constraints()?, dialect)),
                Some(text) => parse_ddl(text, dialect).or_else(|| {
                    let reduced = ddl::split_statements(text, false);
                    parse_ddl(&reduced.first()?.without_constraints()?, dialect)
                }),
            };
            match parsed.and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateTable(create)) => {
                    if !create.columns.is_empty() || postgres {
                        push(OpKind::Table(Box::new(create), likes));
                    }
                }
                Some(_) => {}
                None => skipped.push(("table", stmt.table_name())),
            }
        } else if stmt.creates("domain") {
            let parsed = stmt.domain().and_then(|(name, base)| {
                let name = parse_part(&name)?.parse_object_name(false).ok()?;
                let base = parse_part(&base)?.parse_data_type().ok()?;
                Some((name, base))
            });
            if let Some((name, base)) = parsed {
                let mut checked = None;
                // A collation may equate strings, so the listed values do not bound them.
                let checks = if stmt.has_word("collate") {
                    Vec::new()
                } else {
                    stmt.domain_checks()
                };
                for text in checks {
                    let found = parse_part(&text)
                        .and_then(|mut parser| parser.parse_expr().ok())
                        .and_then(|expr| {
                            values::check_values(&expr, &|ident| {
                                ident.value.eq_ignore_ascii_case("value")
                            })
                        });
                    add_values(&mut checked, found);
                }
                push(OpKind::Domain(name, base, checked));
            }
        } else if stmt.creates("type") {
            if let Some(Statement::CreateType {
                name,
                representation: Some(representation),
            }) = parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next())
            {
                let def = match representation {
                    UserDefinedTypeRepresentation::Enum { labels } => {
                        TypeDef::Enum(labels.into_iter().map(|label| label.value).collect())
                    }
                    UserDefinedTypeRepresentation::Composite { attributes } => TypeDef::Composite(
                        attributes
                            .into_iter()
                            .map(|attr| (fold(&attr.name, dialect), attr.data_type))
                            .collect(),
                    ),
                    _ => continue,
                };
                push(OpKind::Type(name, def));
            }
        } else if stmt.creates("view") {
            match parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateView(view)) => push(OpKind::View(Box::new(view))),
                _ => {
                    skipped.push(("view", stmt.view_name()));
                    if postgres {
                        push(OpKind::Taint(
                            Taint::Relation,
                            stmt.name_parts_after("view"),
                        ));
                    }
                }
            }
        } else if postgres && stmt.creates("function") {
            match parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateFunction(create)) => push(OpKind::Function(Box::new(create))),
                _ => push(OpKind::Taint(
                    Taint::Function,
                    stmt.name_parts_after("function"),
                )),
            }
        } else if matches!(
            dialect,
            SqlDialect::Postgres | SqlDialect::MySql | SqlDialect::Sqlite
        ) {
            read_alter_or_drop(stmt, dialect, &mut push);
        }
    }
    ops
}

pub(super) fn read_alter_or_drop(
    stmt: &ddl::Statement,
    dialect: SqlDialect,
    push: &mut impl FnMut(OpKind),
) {
    // sqlparser rejects `COLLATE` after an ALTER COLUMN type; the collation does not
    // change the type.
    let parsed = || {
        let sql = stmt.sql();
        parse_ddl(&sql, dialect)
            .or_else(|| parse_ddl(&COLLATE.replace_all(&sql, ""), dialect))
            .and_then(|stmts| stmts.into_iter().next())
    };
    if let Some((kind, name, action)) = stmt.target_and_action("alter") {
        match (kind.as_str(), parsed()) {
            ("table", Some(Statement::AlterTable(alter))) => {
                push(OpKind::AlterTable(Box::new(alter)));
            }
            ("table", _) if !changes_columns(&action, stmt.has_word("type")) => {}
            ("table", _) => push(OpKind::Taint(Taint::Relation, name)),
            ("type", Some(Statement::AlterType(alter))) => push(OpKind::AlterType(alter)),
            // A constraint added to or dropped from a domain changes the values it allows.
            ("domain", _) if action.iter().any(|w| w == "constraint") => {
                push(OpKind::Taint(Taint::Type, name));
            }
            ("type", _) if action.first().is_some_and(|w| w == "owner") => {}
            ("type", _) => push(OpKind::Taint(Taint::Type, name)),
            ("view", _) if action.first().is_some_and(|w| w == "rename") => {
                push(OpKind::Taint(Taint::Relation, name));
            }
            ("function", _) if action.first().is_some_and(|w| w == "rename") => {
                push(OpKind::Taint(Taint::Function, name));
            }
            _ => {}
        }
    } else if let Some((kind, name)) = stmt.target("drop") {
        match parsed() {
            Some(
                drop @ (Statement::Drop { .. }
                | Statement::DropFunction(_)
                | Statement::DropDomain(_)),
            ) => {
                push(OpKind::Drop(Box::new(drop)));
            }
            _ if matches!(kind.as_str(), "table" | "view" | "materialized") => {
                push(OpKind::Taint(Taint::Relation, name));
            }
            _ if kind == "type" => push(OpKind::Taint(Taint::Type, name)),
            _ => {}
        }
    }
}

/// Relations PostgreSQL provides, with their PostgreSQL column types.
pub(super) const BUILTIN_SQL: &str = include_str!("builtin_catalog.sql");

/// Read schema DDL into a [`Catalog`], and the `(kind, name)` of the tables and views
/// that could not be parsed. In PostgreSQL the catalog also holds the built-in
/// relations, `search_path` and functions; it, MySQL and SQLite follow `ALTER` and `DROP`.
/// Other dialects read `CREATE TABLE`, `VIEW`, `DOMAIN` and `TYPE` only.
pub(crate) fn load(schema_sql: &str, dialect: &str) -> (Catalog, Vec<SkippedObject>) {
    let dialect = SqlDialect::from_name(dialect);
    let mut loader = Loader::new(dialect);
    let mut skipped = Vec::new();
    if dialect == SqlDialect::Postgres {
        let builtin = ddl::split_statements(BUILTIN_SQL, false);
        let ops = read_statements(&builtin, dialect, &mut loader, &mut Vec::new());
        loader.run(ops);
        loader.search_path = vec!["public".to_string()];
    }
    let statements = ddl::split_statements(schema_sql, dialect == SqlDialect::MySql);
    let ops = read_statements(&statements, dialect, &mut loader, &mut skipped);
    loader.run(ops);
    skipped.append(&mut loader.skipped);
    let mut catalog = loader.build();
    if catalog.search_path.is_empty() {
        catalog.search_path.push("public".to_string());
    }
    (catalog, skipped)
}
