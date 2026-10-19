//! Reading schema statements into loader operations, and the [`load`] entry point.

use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use sqlparser::ast::{
    ColumnOption, ColumnOptionDef, CreateTable, CreateView, Statement,
    UserDefinedTypeRepresentation,
};
use sqlparser::parser::Parser;

use crate::analyzer::{parse_ddl, SkippedObject, SqlDialect};
use crate::ddl;
use crate::values::{self};

use super::alter::changes_columns;
use super::columns::add_values;
use super::loader::{Loader, Op, OpKind, Taint, TypeDef};
use super::names::{fold, parts};
use super::Catalog;
use super::TypeOverrides;

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
        // PostgreSQL and MySQL copy a table's columns with `LIKE`.
        let reads_like = postgres || dialect == SqlDialect::MySql;
        if let Some((name, module, args)) = (dialect == SqlDialect::Sqlite)
            .then(|| stmt.virtual_table())
            .flatten()
        {
            match virtual_table(&name, &module, &args) {
                Some(create) => push(OpKind::Table(Box::new(create), Vec::new())),
                None => skipped.push(("table", name)),
            }
        } else if stmt.is_create_table() {
            let (likes, text) = match reads_like.then(|| stmt.without_like()).flatten() {
                Some((text, likes)) => (likes, Some(text)),
                None => (Vec::new(), None),
            };
            let parsed = match &text {
                None => parse_ddl(&stmt.sql(), dialect)
                    .or_else(|| parse_ddl(&stmt.without_constraints()?, dialect))
                    // `STRICT` and `WITHOUT ROWID` both make a PRIMARY KEY NOT NULL, the
                    // only thing either changes here.
                    .or_else(|| match dialect {
                        SqlDialect::Sqlite => parse_ddl(&stmt.with_first_table_option()?, dialect),
                        _ => None,
                    })
                    .or_else(|| {
                        let text = stmt.without_partitioning()?;
                        parse_ddl(&text, dialect).or_else(|| {
                            let reduced = ddl::split_statements(&text, !postgres);
                            parse_ddl(&reduced.first()?.without_constraints()?, dialect)
                        })
                    }),
                Some(text) => parse_ddl(text, dialect).or_else(|| {
                    let reduced = ddl::split_statements(text, false);
                    parse_ddl(&reduced.first()?.without_constraints()?, dialect)
                }),
            };
            match parsed.and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateTable(create))
                    if create.columns.is_empty()
                        && create.like.is_none()
                        && create.query.is_some() =>
                {
                    let view = create.query.as_ref().and_then(|query| {
                        let sql = format!("CREATE VIEW {} AS {query}", create.name);
                        match parse_ddl(&sql, dialect)?.into_iter().next()? {
                            Statement::CreateView(view) => Some(view),
                            _ => None,
                        }
                    });
                    match view {
                        Some(view) => push(OpKind::View(
                            Box::new(CreateView {
                                if_not_exists: create.if_not_exists,
                                ..view
                            }),
                            true,
                        )),
                        None => skipped.push(("table", stmt.table_name())),
                    }
                }
                Some(Statement::CreateTable(create)) => {
                    if !create.columns.is_empty()
                        || postgres
                        || create.like.is_some()
                        || !likes.is_empty()
                    {
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
            let parsed = parse_ddl(&stmt.sql(), dialect)
                .or_else(|| parse_ddl(&stmt.without_check_option()?, dialect));
            match parsed.and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateView(view)) => push(OpKind::View(Box::new(view), false)),
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
        } else if let Some(trigger) = stmt
            .before_row_trigger()
            .filter(|_| matches!(dialect, SqlDialect::Postgres | SqlDialect::MySql))
        {
            // A SQLite trigger cannot change the row it fires for.
            push(OpKind::RowTrigger(trigger));
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
            .or_else(|| {
                let listed = stmt
                    .respelled_mysql_alter()
                    .filter(|_| dialect == SqlDialect::MySql);
                listed.and_then(|sql| parse_ddl(&sql, dialect))
            })
            .and_then(|stmts| stmts.into_iter().next())
    };
    // MySQL's `RENAME TABLE a TO b, c TO d` renames each table in turn.
    if let (SqlDialect::MySql, Some(_)) = (dialect, stmt.target("rename")) {
        if let Some(Statement::RenameTable(renames)) = parsed() {
            for rename in renames {
                let sql = format!(
                    "ALTER TABLE {} RENAME TO {}",
                    rename.old_name, rename.new_name
                );
                if let Some(Statement::AlterTable(alter)) =
                    parse_ddl(&sql, dialect).and_then(|stmts| stmts.into_iter().next())
                {
                    push(OpKind::AlterTable(Box::new(alter)));
                }
            }
        }
        return;
    }
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
            Some(Statement::DropTrigger(drop)) => push(OpKind::DropTrigger(
                parts(&drop.trigger_name, dialect),
                drop.table_name.map(|table| parts(&table, dialect)),
            )),
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
    load_with(schema_sql, dialect, Arc::default())
}

/// [`load`], with the Python types the project sets for SQL types and columns.
pub(crate) fn load_with(
    schema_sql: &str,
    dialect: &str,
    overrides: Arc<TypeOverrides>,
) -> (Catalog, Vec<SkippedObject>) {
    let dialect = SqlDialect::from_name(dialect);
    let mut loader = Loader::new(dialect);
    loader.overrides = overrides;
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

/// The table an SQLite FTS3, FTS4, FTS5 or R*Tree module declares, `None` for another module.
/// An FTS column takes any value; FTS5's hidden `rank` is a REAL and FTS3/4's `docid` the rowid.
/// An R*Tree has an integer id, coordinates that are never NULL, and `+` auxiliary columns.
fn virtual_table(name: &str, module: &str, args: &[Vec<String>]) -> Option<CreateTable> {
    let module = module.to_ascii_lowercase();
    let column = |arg: &[String]| arg.iter().find(|token| *token != "+").cloned();
    let (columns, hidden) = match module.as_str() {
        "fts3" | "fts4" | "fts5" => {
            let mut columns: Vec<String> = args
                .iter()
                .filter(|arg| {
                    !arg.iter().any(|token| token == "=")
                        && !arg[0].eq_ignore_ascii_case("tokenize")
                })
                .filter_map(|arg| column(arg))
                .collect();
            let hidden = if module == "fts5" {
                "rank REAL"
            } else {
                "docid INTEGER NOT NULL"
            };
            columns.push(hidden.to_string());
            (columns, hidden.split(' ').next())
        }
        "rtree" | "rtree_i32" => {
            let coordinate = if module == "rtree" { "REAL" } else { "INTEGER" };
            let columns = args
                .iter()
                .enumerate()
                .map(|(at, arg)| {
                    let name = column(arg)?;
                    Some(if at == 0 {
                        format!("{name} INTEGER NOT NULL")
                    } else if arg[0] == "+" {
                        name
                    } else {
                        format!("{name} {coordinate} NOT NULL")
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            (columns, None)
        }
        _ => return None,
    };
    let sql = format!("CREATE TABLE {name} ({})", columns.join(", "));
    let Statement::CreateTable(mut create) =
        parse_ddl(&sql, SqlDialect::Sqlite)?.into_iter().next()?
    else {
        return None;
    };
    for column in &mut create.columns {
        if hidden.is_some_and(|hidden| column.name.value.eq_ignore_ascii_case(hidden)) {
            column.options.push(ColumnOptionDef {
                name: None,
                option: ColumnOption::Invisible,
            });
        }
    }
    Some(create)
}
