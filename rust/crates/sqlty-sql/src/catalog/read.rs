//! Reading schema statements into loader operations, and the [`load`] entry point.

use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use sqlparser::ast::{
    ColumnOption, ColumnOptionDef, CreateTable, Statement, UserDefinedTypeRepresentation,
};
use sqlparser::parser::Parser;

use crate::analyzer::{parse_ddl, parse_function_without_hints, SkippedObject, SqlDialect};
use crate::ddl;
use crate::values::{self};

use super::alter::changes_columns;
use super::columns::{add_values, unique_key};
use super::index::NameIndex;
use super::loader::{Loader, Op, OpKind, Taint, TypeDef};
use super::names::{fold, parts};
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

/// What one statement contributes to the catalog, before it meets the loader's state.
#[derive(Default)]
struct Read {
    search_path: Option<Vec<String>>,
    ops: Vec<OpKind>,
    skipped: Vec<SkippedObject>,
}

/// Parse one statement; it depends on the statement and the dialect only.
fn read_statement(stmt: &ddl::Statement<'_>, dialect: SqlDialect) -> Read {
    let postgres = dialect == SqlDialect::Postgres;
    let parser_dialect = dialect.parser_dialect();
    let parse_part = |text: &str| Parser::new(parser_dialect.as_ref()).try_with_sql(text).ok();
    let mut read = Read::default();
    if postgres {
        if let Some(schemas) = stmt.search_path() {
            read.search_path = Some(schemas);
            return read;
        }
    }
    let mut push = |kind: OpKind| read.ops.push(kind);
    // PostgreSQL and MySQL copy a table's columns with `LIKE`.
    let reads_like = postgres || dialect == SqlDialect::MySql;
    if let Some((name, module, args)) = (dialect == SqlDialect::Sqlite)
        .then(|| stmt.virtual_table())
        .flatten()
    {
        match virtual_table(&name, &module, &args) {
            Some(create) => push(OpKind::Table(Box::new(create), Vec::new())),
            None => read.skipped.push(("table", name)),
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
        let parsed =
            parsed
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
            None => read.skipped.push(("table", stmt.table_name())),
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
            read.skipped.push(("statement", statement_text(stmt)));
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
                _ => return read,
            };
            push(OpKind::Type(name, def));
        }
    } else if stmt.creates("view") {
        let parsed = parse_ddl(&stmt.sql(), dialect)
            .or_else(|| parse_ddl(&stmt.without_check_option()?, dialect));
        match parsed.and_then(|stmts| stmts.into_iter().next()) {
            Some(Statement::CreateView(view)) => push(OpKind::View(Box::new(view), false)),
            _ => {
                read.skipped.push(("view", stmt.view_name()));
                if postgres {
                    push(OpKind::Taint(
                        Taint::Relation,
                        stmt.name_parts_after("view"),
                    ));
                }
            }
        }
    } else if postgres && stmt.creates("function") {
        let parsed =
            parse_ddl(&stmt.sql(), dialect).or_else(|| parse_function_without_hints(&stmt.sql()));
        match parsed.and_then(|stmts| stmts.into_iter().next()) {
            Some(Statement::CreateFunction(create)) => push(OpKind::Function(Box::new(create))),
            _ => push(OpKind::Taint(
                Taint::Function,
                stmt.name_parts_after("function"),
            )),
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
        read_alter_or_drop(stmt, dialect, &mut push, &mut read.skipped);
    }
    read
}

/// Statements read together by one runner task.
const BATCH: usize = 16;
/// Statements parsed before the next ones are taken, which bounds the parsed statements alive.
const WINDOW: usize = 256;

/// A run of consecutive statements, read apart from the loader so a runner may read
/// batches on other threads.
pub struct StatementBatch<'a> {
    statements: Vec<ddl::Statement<'a>>,
    dialect: SqlDialect,
    read: Vec<Read>,
}

impl StatementBatch<'_> {
    /// Parse the statements of the batch.
    pub fn read(&mut self) {
        self.read = self
            .statements
            .iter()
            .map(|stmt| read_statement(stmt, self.dialect))
            .collect();
    }
}

/// Reads every batch of a window, in any order and on any thread.
pub type BatchRunner<'r> = &'r (dyn Fn(&mut [StatementBatch<'_>]) + Sync);

/// A [`BatchRunner`] that reads the batches one after another.
pub fn read_serially(batches: &mut [StatementBatch<'_>]) {
    for batch in batches {
        batch.read();
    }
}

/// Apply what a statement was read to, in statement order.
fn apply_read(
    read: Read,
    loader: &mut Loader,
    path: &mut Rc<Vec<String>>,
    pending: &mut Vec<Op>,
    skipped: &mut Vec<SkippedObject>,
) {
    if let Some(schemas) = read.search_path {
        loader.search_path = schemas.clone();
        *path = Rc::new(schemas);
    }
    for kind in read.ops {
        let op = Op {
            path: path.clone(),
            kind,
        };
        if !loader.apply(&op, false) {
            pending.push(op);
        }
    }
    skipped.extend(read.skipped);
}

/// Read the statements and apply what each yields in statement order; the ones waiting on a
/// later definition are returned for [`Loader::run`]. Without a `run`, each statement is read
/// and dropped before the next; with one, the statements are read in windows through it and a
/// window is dropped before the next.
pub(super) fn read_statements<'a>(
    statements: impl IntoIterator<Item = ddl::Statement<'a>>,
    dialect: SqlDialect,
    loader: &mut Loader,
    skipped: &mut Vec<SkippedObject>,
    run: Option<BatchRunner<'_>>,
) -> Vec<Op> {
    let mut path = Rc::new(vec!["public".to_string()]);
    let mut pending = Vec::new();
    let mut statements = statements.into_iter();
    let Some(run) = run else {
        for stmt in statements {
            let read = read_statement(&stmt, dialect);
            apply_read(read, loader, &mut path, &mut pending, skipped);
        }
        return pending;
    };
    loop {
        let window: Vec<_> = statements.by_ref().take(WINDOW).collect();
        if window.is_empty() {
            break;
        }
        let reads: Vec<Read> = if window.len() <= BATCH {
            window
                .iter()
                .map(|stmt| read_statement(stmt, dialect))
                .collect()
        } else {
            let mut batches = Vec::new();
            let mut rest = window.into_iter();
            loop {
                let statements: Vec<_> = rest.by_ref().take(BATCH).collect();
                if statements.is_empty() {
                    break;
                }
                batches.push(StatementBatch {
                    statements,
                    dialect,
                    read: Vec::new(),
                });
            }
            run(&mut batches);
            batches.into_iter().flat_map(|batch| batch.read).collect()
        };
        for read in reads {
            apply_read(read, loader, &mut path, &mut pending, skipped);
        }
    }
    pending
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
            ("table", _) if changes_columns(&action, stmt.has_word("type")) => {
                push(OpKind::Taint(Taint::Relation, name));
            }
            ("table", _) if KEYS_AND_NULLS.iter().any(|word| stmt.has_word(word)) => {
                skipped.push(("statement", statement_text(stmt)));
            }
            ("table", _) => {}
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
    load_with(schema_sql, dialect, Arc::default(), None)
}

/// [`load`], with the Python types the project sets for SQL types and columns, and the
/// runner that reads the statements in windows, if any.
pub(crate) fn load_with(
    schema_sql: &str,
    dialect: &str,
    overrides: Arc<TypeOverrides>,
    run: Option<BatchRunner<'_>>,
) -> (Catalog, Vec<SkippedObject>) {
    // The built-ins are read once per process: parsing them costs more than most schemas.
    static BUILTIN: LazyLock<Loader> = LazyLock::new(|| {
        let mut loader = Loader::new(SqlDialect::Postgres);
        let builtin = ddl::statements(BUILTIN_SQL, false);
        let ops = read_statements(
            builtin,
            SqlDialect::Postgres,
            &mut loader,
            &mut Vec::new(),
            None,
        );
        loader.run(ops);
        loader.search_path = vec!["public".to_string()];
        loader
    });
    let dialect = SqlDialect::from_name(dialect);
    let mut loader = if dialect == SqlDialect::Postgres {
        BUILTIN.fork()
    } else {
        Loader::new(dialect)
    };
    loader.overrides = overrides;
    let mut skipped = Vec::new();
    let statements = ddl::statements(schema_sql, dialect == SqlDialect::MySql);
    let ops = read_statements(statements, dialect, &mut loader, &mut skipped, run);
    loader.run(ops);
    skipped.append(&mut loader.skipped);
    let mut catalog = loader.build();
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::thread;

    use super::*;
    use crate::analyzer::{analyze_select, load_schema_in_batches, load_schema_with_overrides};

    fn assert_send<T: Send>() {}

    /// `count` statements mixing every kind the reader handles, with a forward reference
    /// that spans the whole schema and a `search_path` change at a window boundary.
    fn schema(count: usize) -> String {
        let mut statements: Vec<String> = (0..count)
            .map(|i| {
                let group = i / 12;
                match i % 12 {
                    0 => format!("CREATE TABLE t{group} (id int PRIMARY KEY, name text);"),
                    1 => format!("SET search_path = sch{}, public;", group % 3),
                    2 => format!("CREATE VIEW v{group} AS SELECT id FROM t{group};"),
                    3 => format!("CREATE DOMAIN d{group} AS int CHECK (VALUE > 0);"),
                    4 => format!("CREATE TYPE e{group} AS ENUM ('a', 'b');"),
                    5 => format!("CREATE TYPE c{group} AS (x int, y text);"),
                    6 => format!(
                        "CREATE FUNCTION f{group}() RETURNS int LANGUAGE sql AS 'SELECT 1';"
                    ),
                    7 => format!("ALTER TABLE t{group} ADD COLUMN extra int;"),
                    8 if group % 2 == 0 => format!("DROP TABLE t{group};"),
                    8 => format!("ALTER TABLE t{group} ALTER COLUMN extra SET NOT NULL;"),
                    9 => format!("CREATE TABLE broken{group} (id int,, );"),
                    10 => format!("CREATE VIEW fv{group} AS SELECT id FROM late{group};"),
                    _ => format!("CREATE TABLE late{group} (id int);"),
                }
            })
            .collect();
        if count > 1 {
            statements[0] = "CREATE VIEW first AS SELECT id FROM last_t;".to_string();
            statements[count - 1] = "CREATE TABLE last_t (id int);".to_string();
        }
        if count > 256 {
            statements[255] = "SET search_path = sch1, public;".to_string();
            statements[256] = "CREATE TABLE edge_t (id int);".to_string();
        }
        statements.join("\n")
    }

    /// Every probe's columns and the skipped list, as one comparable value.
    fn load_and_probe(sql: &str, run: Option<BatchRunner<'_>>) -> String {
        let (schema, skipped) = match run {
            Some(run) => load_schema_in_batches(sql, "postgres", Default::default(), run),
            None => load_schema_with_overrides(sql, "postgres", Default::default()),
        };
        let mut seen = format!("{skipped:?}\n");
        let names = ["first", "last_t", "edge_t"]
            .map(String::from)
            .into_iter()
            .chain((0..50).flat_map(|g| ["t", "v", "fv", "late"].map(|kind| format!("{kind}{g}"))));
        for name in names {
            for prefix in ["", "public.", "sch0.", "sch1.", "sch2."] {
                let probe = format!("SELECT * FROM {prefix}{name}");
                let columns = analyze_select(&probe, "postgres", Some(&schema));
                seen.push_str(&format!("{probe}: {columns:?}\n"));
            }
        }
        seen
    }

    fn reversed(batches: &mut [StatementBatch<'_>]) {
        for batch in batches.iter_mut().rev() {
            batch.read();
        }
    }

    fn threaded(batches: &mut [StatementBatch<'_>]) {
        thread::scope(|scope| {
            for batch in batches.iter_mut() {
                scope.spawn(|| batch.read());
            }
        });
    }

    #[test]
    fn a_batch_can_be_read_on_another_thread() {
        assert_send::<StatementBatch<'static>>();
    }

    #[test]
    fn every_runner_loads_the_same_catalog_across_window_boundaries() {
        for count in [0, 1, 2, 16, 17, 255, 256, 257, 513, 700] {
            let sql = schema(count);
            let streamed = load_and_probe(&sql, None);
            let serial = load_and_probe(&sql, Some(&read_serially));
            assert_eq!(streamed, serial, "{count} serial");
            let reverse = load_and_probe(&sql, Some(&reversed));
            assert_eq!(streamed, reverse, "{count} reversed");
            let threads = load_and_probe(&sql, Some(&threaded));
            assert_eq!(streamed, threads, "{count} threaded");
        }
    }

    #[test]
    fn the_mixed_schema_exercises_what_the_runners_must_agree_on() {
        let seen = load_and_probe(&schema(700), None);
        assert!(seen.contains("(\"table\", \"broken0\")"));
        assert!(seen.contains("SELECT * FROM first: Some(["));
        assert!(seen.contains("SELECT * FROM sch1.edge_t: Some(["));
        assert!(seen.contains("SELECT * FROM v1: Some(["));
        assert!(seen.contains("SELECT * FROM sch1.t2: Some([Column { name: \"id\""));
        assert!(seen.contains("SELECT * FROM sch2.t2: Some([Column { name: \"*\""));
    }
}
