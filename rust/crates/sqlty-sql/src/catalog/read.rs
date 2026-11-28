//! Reading schema statements into loader operations, and the [`load`] entry point.

use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use rayon::prelude::*;
use sqlparser::ast::{
    ColumnOption, ColumnOptionDef, CommentObject, CreateTable, Statement,
    UserDefinedTypeRepresentation,
};
use sqlparser::parser::Parser;

use crate::analyzer::{parse_ddl, parse_function_without_hints, SkippedObject, SqlDialect};
use crate::ddl;
use crate::values::{self};

use super::alter::changes_columns;
use super::columns::{add_values, unique_key};
use super::index::NameIndex;
use super::loader::{Commented, Loader, Op, OpKind, Taint, TypeDef};
use super::names::{fold, parts};
use super::policies::Change;
use super::Catalog;
use super::TypeOverrides;

pub(super) static COLLATE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::regex(r#"(?i)\s+COLLATE\s+(?:"[^"]*"|[\w.]+)"#));

/// The sequence options of `ADD GENERATED ... AS IDENTITY (...)`, which pg_dump writes.
static IDENTITY_OPTIONS: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::regex(r"(?i)\bAS\s+IDENTITY\s*\([^()]*\)"));

/// Words of an `ALTER TABLE` that sets what inference reads besides the columns: keys,
/// checks, `NOT NULL` and defaults.
const KEYS_AND_NULLS: [&str; 7] = [
    "constraint",
    "primary",
    "unique",
    "foreign",
    "check",
    "null",
    "default",
];

/// A statement on one line, cut to fit a warning.
fn statement_text(stmt: &ddl::Statement) -> String {
    const WIDTH: usize = 80;
    let text = stmt.sql().split_whitespace().collect::<Vec<_>>().join(" ");
    match text.char_indices().nth(WIDTH) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text,
    }
}

/// The column list of PostgreSQL 15's `ON DELETE SET NULL | DEFAULT (cols)`, which
/// sqlparser does not read; it names the columns the action sets, not the key.
static FK_ACTION_COLUMNS: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::regex(r"(?i)\bON\s+DELETE\s+SET\s+(NULL|DEFAULT)\s*\([^()]*\)"));

static DROP_EXPRESSION: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::regex(r"(?i)\bDROP\s+EXPRESSION(?:\s+IF\s+EXISTS)?\b"));

/// SQL bytes of the statements parsed together while the batch before them is applied.
const BATCH_BYTES: usize = 1 << 14;

/// What one statement reads as: the operations it adds and what it skips, or the
/// `search_path` it sets.
struct Read<'a> {
    stmt: ddl::Statement<'a>,
    ops: Vec<OpKind>,
    skipped: Vec<SkippedObject>,
    search_path: Option<Vec<String>>,
}

impl<'a> Read<'a> {
    fn new(stmt: ddl::Statement<'a>, dialect: SqlDialect) -> Self {
        let (mut ops, mut skipped) = (Vec::new(), Vec::new());
        let search_path = read_statement(&stmt, dialect, &mut |op| ops.push(op), &mut skipped);
        Self {
            stmt,
            ops,
            skipped,
            search_path,
        }
    }
}

/// Apply each statement in order; the ones waiting on a later definition are returned
/// for [`Loader::run`]. When `parallel` on a rayon thread, batches of statements are
/// parsed on the pool while the batch before them is applied, so at most two batches of
/// parsed statements are held; otherwise a statement lives only until it is applied.
/// A thread waiting on the pool runs other jobs, so `parallel` is only for a caller
/// that holds no lock and runs inside no lazy initializer another job may wait on.
pub(super) fn read_statements<'a>(
    source: &'a str,
    statements: impl IntoIterator<Item = ddl::Statement<'a>, IntoIter: Send>,
    dialect: SqlDialect,
    loader: &mut Loader,
    skipped: &mut Vec<SkippedObject>,
    parallel: bool,
) -> Vec<Op> {
    let mut path = Rc::new(vec!["public".to_string()]);
    let mut pending = Vec::new();
    let mut apply = |read: Read| {
        if loader.exhausted() {
            skipped.push(("limit", format!("{} and later", statement_text(&read.stmt))));
            return false;
        }
        if let Some(schemas) = read.search_path {
            loader.set_search_path(schemas.clone());
            path = Rc::new(schemas);
            return true;
        }
        let (size, at) = (
            read.stmt.size(),
            read.stmt.range_in(source).unwrap_or_default(),
        );
        for kind in read.ops {
            let op = Op {
                path: path.clone(),
                kind,
                size,
                at: at.clone(),
            };
            if !loader.apply(&op, false) {
                pending.push(op);
            }
        }
        skipped.extend(read.skipped);
        true
    };
    let mut statements = statements.into_iter();
    if !parallel || rayon::current_thread_index().is_none() {
        for stmt in statements {
            if !apply(Read::new(stmt, dialect)) {
                break;
            }
        }
        return pending;
    }
    let mut parsed = read_batch(&mut statements, dialect);
    let mut reading = true;
    while reading && !parsed.is_empty() {
        let mut next = Vec::new();
        reading = rayon::in_place_scope(|scope| {
            scope.spawn(|_| next = read_batch(&mut statements, dialect));
            parsed.into_iter().all(&mut apply)
        });
        parsed = next;
    }
    pending
}

/// The next statements of about [`BATCH_BYTES`], read on the pool.
fn read_batch<'a>(
    statements: &mut impl Iterator<Item = ddl::Statement<'a>>,
    dialect: SqlDialect,
) -> Vec<Read<'a>> {
    let mut batch = Vec::new();
    let mut bytes = 0;
    while bytes < BATCH_BYTES {
        let Some(stmt) = statements.next() else { break };
        bytes += stmt.size();
        batch.push(stmt);
    }
    batch
        .into_par_iter()
        .map(|stmt| Read::new(stmt, dialect))
        .collect()
}

/// Read `stmt` into operations, given to `push`, and what it skips; the `search_path`
/// it sets instead, when it sets one.
fn read_statement(
    stmt: &ddl::Statement,
    dialect: SqlDialect,
    push: &mut impl FnMut(OpKind),
    skipped: &mut Vec<SkippedObject>,
) -> Option<Vec<String>> {
    let postgres = dialect == SqlDialect::Postgres;
    if postgres {
        if let Some(schemas) = stmt.search_path() {
            return Some(schemas);
        }
    }
    let parser_dialect = dialect.parser_dialect();
    let parse_part = |text: &str| Parser::new(parser_dialect.as_ref()).try_with_sql(text).ok();
    {
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
                    .or_else(|| parse_ddl(&without_fk_action_columns(&stmt.sql()), dialect))
                    .or_else(|| parse_ddl(&stmt.without_constraints()?, dialect))
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
            // `CREATE TABLE ... AS query` without columns reads like a view of the query.
            let parsed = parsed
                .and_then(|stmts| stmts.into_iter().next())
                .and_then(|stmt| match stmt {
                    Statement::CreateTable(create)
                        if create.columns.is_empty()
                            && create.like.is_none()
                            && create.query.is_some() =>
                    {
                        let query = create.query.as_ref()?;
                        let exists = if create.if_not_exists {
                            "IF NOT EXISTS "
                        } else {
                            ""
                        };
                        let sql = format!("CREATE VIEW {exists}{} AS {query}", create.name);
                        parse_ddl(&sql, dialect)?.into_iter().next()
                    }
                    stmt => Some(stmt),
                });
            match parsed {
                Some(Statement::CreateView(view)) => push(OpKind::View(Box::new(view), true)),
                Some(Statement::CreateTable(create))
                    if !create.columns.is_empty()
                        || postgres
                        || create.like.is_some()
                        || !likes.is_empty() =>
                {
                    push(OpKind::Table(Box::new(create), likes));
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
            let parsed = parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next());
            if parsed.is_none() {
                skipped.push(("statement", statement_text(stmt)));
            }
            if let Some(Statement::CreateType {
                name,
                representation: Some(representation),
            }) = parsed
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
                    _ => return None,
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
            let parsed = parse_ddl(&stmt.sql(), dialect)
                .or_else(|| parse_function_without_hints(&stmt.sql()));
            match parsed.and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreateFunction(create)) => push(OpKind::Function(Box::new(create))),
                _ => push(OpKind::Taint(
                    Taint::Function,
                    stmt.name_parts_after("function"),
                )),
            }
        } else if postgres && stmt.creates("policy") {
            match parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next()) {
                Some(Statement::CreatePolicy(create)) => {
                    let table = parts(&create.table_name, dialect);
                    push(OpKind::Policy(table, Box::new(Change::Create(create))));
                }
                _ => {
                    skipped.push(("statement", statement_text(stmt)));
                    let table = stmt.name_parts_after("on");
                    push(OpKind::Policy(table, Box::new(Change::Unknown)));
                }
            }
        } else if (postgres || dialect == SqlDialect::Other("duckdb"))
            && stmt.target("comment").is_some()
        {
            // DuckDB writes `COMMENT ON` as PostgreSQL does; its sqlparser dialect does not
            // read it.
            if let Some(Statement::Comment {
                object_type,
                object_name,
                comment,
                ..
            }) = parse_ddl(&stmt.sql(), SqlDialect::Postgres)
                .and_then(|stmts| stmts.into_iter().next())
            {
                let mut name = parts(&object_name, dialect);
                let target = match object_type {
                    CommentObject::Table
                    | CommentObject::View
                    | CommentObject::MaterializedView
                    | CommentObject::ForeignTable => Some(Commented::Relation(name)),
                    CommentObject::Column => name
                        .pop()
                        .filter(|_| !name.is_empty())
                        .map(|column| Commented::Column(name, column)),
                    _ => None,
                };
                let text = comment.filter(|text| !text.is_empty());
                let text = text.map(String::into_boxed_str);
                (target.into_iter()).for_each(|target| push(OpKind::Comment(target, text.clone())));
            }
        } else if stmt.creates("index") {
            if let Some(Statement::CreateIndex(index)) =
                parse_ddl(&stmt.sql(), dialect).and_then(|stmts| stmts.into_iter().next())
            {
                let name = index
                    .name
                    .as_ref()
                    .and_then(|name| name.0.last()?.as_ident());
                let key = unique_key(name, &index.columns, dialect);
                if let (true, None, Some(key)) = (index.unique, &index.predicate, key) {
                    push(OpKind::UniqueIndex(parts(&index.table_name, dialect), key));
                }
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
            read_alter_or_drop(stmt, dialect, push, skipped);
        }
    }
    None
}

fn without_fk_action_columns(sql: &str) -> String {
    FK_ACTION_COLUMNS
        .replace_all(sql, "ON DELETE SET $1")
        .into_owned()
}

pub(super) fn read_alter_or_drop(
    stmt: &ddl::Statement,
    dialect: SqlDialect,
    push: &mut impl FnMut(OpKind),
    skipped: &mut Vec<SkippedObject>,
) {
    // sqlparser rejects `COLLATE` after an ALTER COLUMN type; the collation does not
    // change the type.
    let parsed = || {
        let sql = stmt.sql();
        parse_ddl(&sql, dialect)
            .or_else(|| parse_ddl(&COLLATE.replace_all(&sql, ""), dialect))
            .or_else(|| parse_ddl(&without_fk_action_columns(&sql), dialect))
            // `DROP EXPRESSION` leaves a column with neither expression nor default.
            .or_else(|| parse_ddl(&DROP_EXPRESSION.replace_all(&sql, "DROP DEFAULT"), dialect))
            // sqlparser rejects most of them; they do not change the column.
            .or_else(|| parse_ddl(&IDENTITY_OPTIONS.replace_all(&sql, "AS IDENTITY"), dialect))
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
            let sql: String = renames
                .iter()
                .map(|r| format!("ALTER TABLE {} RENAME TO {};", r.old_name, r.new_name))
                .collect();
            for alter in ddl::split_statements(&sql, true) {
                read_alter_or_drop(&alter, dialect, push, skipped);
            }
        }
        return;
    }
    if let Some((kind, name, action)) = stmt.target_and_action("alter") {
        match (kind.as_str(), parsed()) {
            ("table", Some(Statement::AlterTable(alter))) => {
                push(OpKind::AlterTable(Box::new(alter)));
            }
            ("table", _)
                if dialect == SqlDialect::MySql
                    && action.first().is_some_and(|w| w == "comment") =>
            {
                // `ALTER TABLE t COMMENT [=] 'text'`, which sqlparser does not read.
                if let Some(text) = stmt.tokens().last().and_then(|quoted| mysql_string(quoted)) {
                    let text = (!text.is_empty()).then(|| text.into_boxed_str());
                    push(OpKind::Comment(Commented::Relation(name), text));
                }
            }
            ("table", _) if changes_columns(&action, stmt.has_word("type")) => {
                push(OpKind::Taint(Taint::Relation, name));
            }
            ("table", _) if KEYS_AND_NULLS.iter().any(|word| stmt.has_word(word)) => {
                skipped.push(("statement", statement_text(stmt)));
            }
            ("table", _) => {}
            ("type", Some(Statement::AlterType(alter))) => push(OpKind::AlterType(alter)),
            ("policy", Some(Statement::AlterPolicy(alter))) if dialect == SqlDialect::Postgres => {
                let table = parts(&alter.table_name, dialect);
                push(OpKind::Policy(table, Box::new(Change::Alter(alter))));
            }
            ("policy", _) if dialect == SqlDialect::Postgres => {
                skipped.push(("statement", statement_text(stmt)));
                let table = stmt.name_parts_after("on");
                push(OpKind::Policy(table, Box::new(Change::Unknown)));
            }
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
            Some(Statement::DropPolicy(drop)) if dialect == SqlDialect::Postgres => {
                let table = parts(&drop.table_name, dialect);
                push(OpKind::Policy(table, Box::new(Change::Drop(drop.name))));
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

/// Relations PostgreSQL provides, with their PostgreSQL column types; the views and hash
/// functions of `pg_catalog` are generated by `scripts/refresh_pg_catalog.py`.
pub(super) const BUILTIN_SQL: &str = concat!(
    include_str!("builtin_catalog.sql"),
    include_str!("pg_catalog_views.sql")
);

/// Read schema DDL into a [`Catalog`], and the `(kind, name)` of the tables and views
/// that could not be parsed. In PostgreSQL the catalog also holds the built-in
/// relations, `search_path` and functions; it, MySQL and SQLite follow `ALTER` and `DROP`.
/// Other dialects read `CREATE TABLE`, `VIEW`, `DOMAIN` and `TYPE` only.
pub(crate) fn load(schema_sql: &str, dialect: &str) -> (Catalog, Vec<SkippedObject>) {
    load_with(schema_sql, dialect, Arc::default(), false)
}

/// [`load`], with the Python types the project sets for SQL types and columns, its
/// statements parsed on the current rayon pool when `parallel` (see [`read_statements`]).
pub(crate) fn load_with(
    schema_sql: &str,
    dialect: &str,
    overrides: Arc<TypeOverrides>,
    parallel: bool,
) -> (Catalog, Vec<SkippedObject>) {
    // The built-ins are read once per process: parsing them costs more than most schemas.
    static BUILTIN: LazyLock<Loader> = LazyLock::new(|| {
        let mut loader = Loader::new(SqlDialect::Postgres);
        let builtin = ddl::statements(BUILTIN_SQL, false);
        let mut skipped = Vec::new();
        let ops = read_statements(
            BUILTIN_SQL,
            builtin,
            SqlDialect::Postgres,
            &mut loader,
            &mut skipped,
            false,
        );
        loader.run(ops);
        loader.search_path = vec!["public".to_string()];
        loader.defined.clear();
        loader
    });
    let dialect = SqlDialect::from_name(dialect);
    let mut loader = if dialect == SqlDialect::Postgres {
        BUILTIN.fork()
    } else {
        Loader::new(dialect)
    };
    loader.overrides = overrides;
    loader.allow(schema_sql);
    let mut skipped = Vec::new();
    let statements = ddl::statements(schema_sql, dialect == SqlDialect::MySql);
    let ops = read_statements(
        schema_sql,
        statements,
        dialect,
        &mut loader,
        &mut skipped,
        parallel,
    );
    loader.run(ops);
    loader.resolve_foreign_keys();
    skipped.append(&mut loader.skipped);
    let mut catalog = loader.build();
    catalog.sources = loader.sources(&catalog);
    if catalog.search_path.is_empty() {
        catalog.search_path.push("public".to_string());
    }
    catalog.index = NameIndex::new(&catalog);
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
    let Some(Statement::CreateTable(mut create)) =
        parse_ddl(&sql, SqlDialect::Sqlite).and_then(|stmts| stmts.into_iter().next())
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

/// The text of the MySQL string literal `quoted`, or `None` when it is not one.
fn mysql_string(quoted: &str) -> Option<String> {
    let quote = quoted.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inner = quoted.strip_prefix(quote)?.strip_suffix(quote)?;
    let mut text = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => text.extend(chars.next()),
            c if c == quote => text.extend(chars.next()),
            c => text.push(c),
        }
    }
    Some(text)
}
