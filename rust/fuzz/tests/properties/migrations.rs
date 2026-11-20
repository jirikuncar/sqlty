//! Migration rules (`sqlty breaking` on new migration statements) over random migrations:
//! they never panic, report spans inside the file in order, ignore comments and blank lines,
//! report nothing for statements the baseline already has, and exempt tables the migration
//! creates while no rows are copied in.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use proptest::prelude::*;
use sqlty_sql::migrations::{check as findings, Finding, Rule};

use crate::draw::{check, Draw};

const BASELINE: &str = "
CREATE TYPE mood AS ENUM ('ok');
CREATE TABLE teams (id BIGINT PRIMARY KEY, name TEXT);
CREATE TABLE users (
    id BIGINT PRIMARY KEY,
    team_id BIGINT REFERENCES teams (id),
    email TEXT NOT NULL,
    mood mood,
    score INT CHECK (score > 0)
);
CREATE UNIQUE INDEX users_email ON users (email);
CREATE TABLE events (at DATE NOT NULL, kind TEXT) PARTITION BY RANGE (at);
CREATE TABLE events_default PARTITION OF events DEFAULT;
CREATE TABLE events_old (at DATE NOT NULL, kind TEXT, CHECK (at < '2020-01-01'));
";

const OLD: [&str; 3] = ["users", "teams", "events_old"];
const NEW: [&str; 2] = ["fresh", "\"Fresh\""];

/// Statements on `{t}`, a table; `{c}` is a column every table has.
#[rustfmt::skip]
const ON_TABLE: &[&str] = &[
    "ALTER TABLE {t} ADD COLUMN extra INT",
    "ALTER TABLE {t} ADD COLUMN extra INT NOT NULL",
    "ALTER TABLE {t} ADD COLUMN extra INT NOT NULL DEFAULT 0",
    "ALTER TABLE {t} ADD COLUMN extra BIGINT GENERATED ALWAYS AS IDENTITY",
    "ALTER TABLE {t} ADD COLUMN extra mood DEFAULT 'ok'",
    "ALTER TABLE {t} ALTER COLUMN {c} TYPE TEXT",
    "ALTER TABLE {t} ALTER COLUMN {c} TYPE VARCHAR(10)",
    "ALTER TABLE {t} ALTER COLUMN {c} SET NOT NULL",
    "ALTER TABLE {t} ALTER COLUMN {c} DROP NOT NULL",
    "ALTER TABLE {t} ADD CONSTRAINT c_check CHECK ({c} IS NOT NULL) NOT VALID",
    "ALTER TABLE {t} VALIDATE CONSTRAINT c_check",
    "ALTER TABLE {t} ADD CONSTRAINT c_check CHECK ({c} IS NOT NULL)",
    "ALTER TABLE {t} ADD CONSTRAINT c_fk FOREIGN KEY ({c}) REFERENCES teams (id)",
    "ALTER TABLE {t} ADD CONSTRAINT c_fk FOREIGN KEY ({c}) REFERENCES teams (id) NOT VALID",
    "ALTER TABLE {t} ADD UNIQUE ({c})",
    "ALTER TABLE {t} ADD PRIMARY KEY ({c})",
    "ALTER TABLE {t} DROP CONSTRAINT {t}_pkey",
    "ALTER TABLE {t} DROP COLUMN {c}",
    "ALTER TABLE {t} RENAME TO renamed",
    "ALTER TABLE {t} REPLICA IDENTITY FULL",
    "ALTER TABLE {t} REPLICA IDENTITY NOTHING",
    "ALTER TABLE {t} REPLICA IDENTITY DEFAULT",
    "ALTER TABLE {t} DISABLE TRIGGER ALL",
    "ALTER TABLE {t} SET (fillfactor = 70)",
    "ALTER TABLE {t} SET TABLESPACE slow",
    "CREATE INDEX ON {t} ({c})",
    "CREATE INDEX CONCURRENTLY ON {t} ({c})",
    "CREATE INDEX CONCURRENTLY IF NOT EXISTS i ON {t} ({c})",
    "CREATE UNIQUE INDEX i ON {t} ({c})",
    "DROP INDEX i",
    "DROP INDEX CONCURRENTLY i",
    "REINDEX TABLE {t}",
    "CLUSTER {t}",
    "VACUUM FULL {t}",
    "LOCK TABLE {t}",
    "LOCK TABLE {t} IN SHARE MODE",
    "TRUNCATE {t}",
    "TRUNCATE {t} CASCADE",
    "UPDATE {t} SET {c} = {c}",
    "UPDATE {t} SET {c} = {c} WHERE {c} BETWEEN 1 AND 100",
    "UPDATE {t} SET {c} = {c} WHERE id IN (SELECT id FROM {t} LIMIT 100)",
    "DELETE FROM {t} WHERE {c} IS NULL",
    "INSERT INTO {t} SELECT * FROM users",
    "ALTER TABLE events ATTACH PARTITION {t} FOR VALUES FROM ('2019-01-01') TO ('2020-01-01')",
    "ALTER TABLE events DETACH PARTITION {t}",
    "ALTER TABLE events DETACH PARTITION {t} CONCURRENTLY",
    "DROP TABLE {t}",
    "DROP TABLE {t} CASCADE",
];

/// Statements on no table of their own.
#[rustfmt::skip]
const OTHER: &[&str] = &[
    "ALTER TYPE mood ADD VALUE 'meh'",
    "ALTER TYPE mood ADD VALUE IF NOT EXISTS 'meh' BEFORE 'ok'",
    "UPDATE users SET mood = 'meh'",
    "SELECT 'meh'::mood",
    "CREATE TYPE color AS ENUM ('red')",
    "SET lock_timeout = '1s'",
    "SET LOCAL lock_timeout = '1s'",
    "RESET lock_timeout",
    "SET statement_timeout = 0",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "SAVEPOINT s",
    "ROLLBACK TO SAVEPOINT s",
    "CREATE TABLE copy (LIKE users INCLUDING ALL)",
    "CREATE TABLE events_2021 PARTITION OF events FOR VALUES FROM ('2021-01-01') TO ('2022-01-01')",
    "CREATE MATERIALIZED VIEW mv AS SELECT * FROM users",
    "REFRESH MATERIALIZED VIEW mv",
    "SELECT 1",
];

/// Columns every table of [`OLD`] and [`NEW`] has.
const COLUMNS: [&str; 2] = ["id", "team_id"];

fn on(draw: &mut Draw<'_>, tables: &[&str]) -> String {
    draw.pick(ON_TABLE)
        .replace("{t}", draw.pick(tables))
        .replace("{c}", draw.pick(&COLUMNS))
}

/// A migration of up to 16 statements on the baseline's tables, new ones and none.
fn migration(draw: &mut Draw<'_>) -> Vec<String> {
    let mut statements = Vec::new();
    for table in NEW {
        if draw.bool() {
            statements.push(format!(
                "CREATE TABLE {table} (id BIGINT PRIMARY KEY, team_id BIGINT REFERENCES teams (id))"
            ));
        }
    }
    let tables: Vec<&str> = OLD.iter().chain(&NEW).copied().collect();
    statements.extend(draw.list(1, 16, |draw| match draw.int(0, 2) {
        0 => draw.pick(OTHER).to_string(),
        _ => on(draw, &tables),
    }));
    statements
}

fn file(statements: &[String], separator: &str) -> String {
    statements
        .iter()
        .map(|statement| format!("{statement};"))
        .collect::<Vec<_>>()
        .join(separator)
}

fn rules(found: &[Finding]) -> Vec<Rule> {
    found.iter().map(|finding| finding.rule).collect()
}

/// Spans inside `sql`, on character boundaries, in order.
fn spans_hold(sql: &str, found: &[Finding]) -> Result<(), TestCaseError> {
    for finding in found {
        let span = &finding.span;
        prop_assert!(
            span.start <= span.end && span.end <= sql.len(),
            "{finding:?}"
        );
        prop_assert!(sql.is_char_boundary(span.start) && sql.is_char_boundary(span.end));
    }
    prop_assert!(found.is_sorted_by_key(|finding| finding.span.start));
    Ok(())
}

#[test]
fn migration_findings_are_stable() {
    check(
        |draw| {
            (
                migration(draw),
                draw.pick(&["\n", "\n\n-- note\n", " /* c */ "]),
            )
        },
        |(statements, separator)| {
            let sql = file(&statements, "\n");
            let found = findings(BASELINE, &[&sql]).remove(0);
            spans_hold(&sql, &found)?;
            prop_assert_eq!(&found, &findings(BASELINE, &[&sql]).remove(0));
            let spaced = file(&statements, separator);
            prop_assert_eq!(
                rules(&found),
                rules(&findings(BASELINE, &[&spaced]).remove(0))
            );
            let applied = format!("{BASELINE}\n{sql}");
            prop_assert_eq!(findings(&applied, &[&sql]), [Vec::new()]);
            Ok(())
        },
    );
}

/// Rules that judge a statement whatever the rows of its table.
const ANY_TABLE: [Rule; 4] = [
    Rule::ConcurrentlyInTransaction,
    Rule::EnumValueInTransaction,
    Rule::ConcurrentIndexIfNotExists,
    Rule::MissingLockTimeout,
];

#[test]
fn tables_the_migration_creates_are_exempt() {
    check(
        |draw| {
            let table = draw.pick(&NEW);
            let mut statements = vec![
                format!("CREATE TABLE {table} (id BIGINT PRIMARY KEY, team_id BIGINT)"),
                format!("CREATE INDEX i ON {table} (id)"),
            ];
            statements.extend(
                draw.list(1, 12, |draw| match draw.int(0, 3) {
                    0 => draw
                        .pick(&["BEGIN", "COMMIT", "SET LOCAL lock_timeout = '1s'"])
                        .to_string(),
                    _ => on(draw, &[table]),
                })
                .into_iter()
                .filter(|statement| {
                    !statement.contains("events") && !statement.starts_with("INSERT")
                }),
            );
            statements
        },
        |statements| {
            let sql = file(&statements, "\n");
            let found = findings(BASELINE, &[&sql]).remove(0);
            let exempt: Vec<_> = rules(&found)
                .into_iter()
                .filter(|rule| !ANY_TABLE.contains(rule))
                .collect();
            prop_assert!(exempt.is_empty(), "{exempt:?} in\n{sql}");
            Ok(())
        },
    );
}

/// SQL-ish tokens, broken quoting and keywords out of place included.
#[rustfmt::skip]
const TOKENS: &[&str] = &[
    "ALTER", "TABLE", "INDEX", "TYPE", "ADD", "DROP", "VALUE", "CONSTRAINT", "COLUMN", "CREATE", "UNIQUE",
    "CONCURRENTLY", "IF", "NOT", "EXISTS", "ON", "users", "teams", "events", "\"users\"", "public.users",
    "PRIMARY", "KEY", "REFERENCES", "CHECK", "VALID", "VALIDATE", "ATTACH", "DETACH", "PARTITION", "OF",
    "FOR", "VALUES", "FROM", "TO", "DEFAULT", "REPLICA", "IDENTITY", "USING", "FULL", "NOTHING", "RENAME",
    "TRUNCATE", "CASCADE", "UPDATE", "SET", "WHERE", "DELETE", "INSERT", "INTO", "LOCK", "IN", "MODE",
    "BEGIN", "COMMIT", "ROLLBACK", "LOCAL", "lock_timeout", "=", "'1s'", "(", ")", ",", ";", ";", "::",
    "'meh'", "mood", "id", "AS", "LIKE", "INCLUDING", "ALL", "TRIGGER", "DISABLE", "WITH", "SELECT", "*",
    "LIMIT", "BETWEEN", "AND", "OR", "1", "--", "/*", "*/", "'", "\"", "$$", "é", "\u{0}", "\n", "BEGIN ATOMIC",
];

#[test]
fn any_token_soup_is_judged_without_panicking() {
    check(
        |draw| draw.list(0, 60, |draw| draw.pick(TOKENS)).join(" "),
        |sql| {
            let found = findings(BASELINE, &[&sql, &sql]);
            for found in &found {
                spans_hold(&sql, found)?;
            }
            findings(&sql, &[BASELINE]);
            Ok(())
        },
    );
}

static SERVER: LazyLock<Option<Mutex<postgres::Client>>> = LazyLock::new(|| {
    let dsn = std::env::var("SQLTY_PG_DSN").ok()?;
    let client =
        postgres::Client::connect(&dsn, postgres::NoTls).expect("connects to SQLTY_PG_DSN");
    Some(Mutex::new(client))
});

/// The baseline's tables with rows, as `oid`s that outlive a rename or a drop.
const WITH_ROWS: &str = "SELECT string_agg(oid::text, ',') FROM pg_class
    WHERE relnamespace = 'migration'::regnamespace
    AND relname IN ('users', 'teams', 'events_old', 'events_default')";

/// The tables of `WITH_ROWS` this session holds a lock on that blocks writes.
const WRITE_LOCKS: &str = "SELECT DISTINCT relation::oid::text FROM pg_locks
    WHERE pid = pg_backend_pid() AND relation::oid::text = ANY(string_to_array($1, ','))
    AND mode IN ('ShareLock', 'ShareRowExclusiveLock', 'ExclusiveLock', 'AccessExclusiveLock')";

/// What PostgreSQL did with a migration: the statements it ran, up to the first that failed,
/// the SQLSTATE of that failure, and whether a statement of a transaction locked a table with
/// rows against writes while an earlier one held another.
struct Ran {
    statements: usize,
    error: Option<String>,
    two_tables: bool,
}

fn run_on_server(client: &mut postgres::Client, statements: &[String]) -> Ran {
    client
        .batch_execute(
            "ROLLBACK; DROP SCHEMA IF EXISTS migration CASCADE; CREATE SCHEMA migration;
             SET search_path TO migration",
        )
        .expect("a fresh schema");
    client
        .batch_execute(&format!(
            "{BASELINE}
            INSERT INTO teams VALUES (1, 'a');
            INSERT INTO users VALUES (1, 1, 'a@b', 'ok', 1);
            INSERT INTO events VALUES ('2025-01-01', 'a');
            INSERT INTO events_old VALUES ('2019-06-01', 'a');"
        ))
        .expect("the baseline runs");
    let mut ran = Ran {
        statements: 0,
        error: None,
        two_tables: false,
    };
    let tables: String = client
        .query_one(WITH_ROWS, &[])
        .expect("reads pg_class")
        .get(0);
    let mut held = HashSet::new();
    for statement in statements {
        ran.statements += 1;
        if let Err(error) = client.batch_execute(statement) {
            ran.error = error.code().map(|code| code.code().to_string());
            return ran;
        }
        let locked: HashSet<String> = client
            .query(WRITE_LOCKS, &[&tables])
            .expect("reads pg_locks")
            .iter()
            .map(|row| row.get(0))
            .collect();
        ran.two_tables |= !held.is_empty() && !locked.is_subset(&held);
        held = locked;
    }
    ran
}

/// With `SQLTY_PG_DSN`, PostgreSQL runs each migration: `concurrently-in-transaction` fires
/// exactly when it refuses a `CONCURRENTLY` statement in a transaction, `enum-value-in-transaction` whenever it
/// refuses a new enum value, and `multi-table-lock` exactly when a transaction holds write
/// locks on two tables with rows.
#[test]
fn server_agrees_on_migrations() {
    let Some(server) = SERVER.as_ref() else {
        return eprintln!("server_agrees_on_migrations: set SQLTY_PG_DSN to compare with a server");
    };
    check(
        |draw| {
            draw.list(1, 12, |draw| match draw.int(0, 4) {
                0 => draw
                    .pick(&[
                        "BEGIN",
                        "COMMIT",
                        "ROLLBACK",
                        "SET LOCAL lock_timeout = '1s'",
                    ])
                    .to_string(),
                1 => draw.pick(OTHER).to_string(),
                _ => on(draw, &OLD),
            })
        },
        |statements| {
            let mut client = server
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let ran = run_on_server(&mut client, &statements);
            let sql = file(&statements[..ran.statements], "\n");
            let found = rules(&findings(BASELINE, &[&sql]).remove(0));
            let error = ran.error.as_deref();
            prop_assert_eq!(
                found.contains(&Rule::ConcurrentlyInTransaction),
                error == Some("25001") && statements[ran.statements - 1].contains("CONCURRENTLY"),
                "{:?} {:?} in\n{}",
                error,
                found,
                sql
            );
            if error == Some("55P04") {
                prop_assert!(
                    found.contains(&Rule::EnumValueInTransaction),
                    "{found:?} in\n{sql}"
                );
            }
            if error.is_none() {
                prop_assert_eq!(
                    found.contains(&Rule::MultiTableLock),
                    ran.two_tables,
                    "{:?} in\n{}",
                    found,
                    sql
                );
            }
            Ok(())
        },
    );
}
