//! Query parameters: placeholders, call diagnostics and `NoParams`. SQLite is the oracle: it
//! binds a placeholder from a mapping by name and ignores extra keys.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use proptest::prelude::*;
use regex::Regex;
use sqlty_sql::analyzer::Schema;

use crate::draw::{check, Draw};
use crate::{engine, python};

const SCHEMA: &str =
    "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);";
const NAMES: [&str; 6] = ["id", "name", "age", "a", "b_2", "_x"];
const SIGILS: [&str; 3] = [":", "@", "$"];
/// Text that looks like a placeholder but is not one: in a literal, a comment, a cast or a name.
const NOISE: [&str; 5] = [
    "'@a :b $c ?'",
    "\"name\"",
    "1",
    "name || ':x'",
    "CAST(age AS INTEGER)",
];

static SQLITE_SCHEMA: LazyLock<Schema> = LazyLock::new(|| engine::schema(SCHEMA, "sqlite"));
static POSTGRES_SCHEMA: LazyLock<Schema> = LazyLock::new(|| engine::schema(SCHEMA, "postgres"));

type Names = BTreeSet<String>;

/// A SQLite query with named placeholders in any sigil, and the names it binds.
fn named_query(d: &mut Draw<'_>) -> (String, Names) {
    let names = d.list(0, 4, |d| d.pick(&NAMES));
    let mut terms: Vec<String> = names
        .iter()
        .map(|name| format!("id = {}{name}", d.pick(&SIGILS)))
        .collect();
    terms.extend(d.list(0, 2, |d| format!("{} IS NOT NULL", d.pick(&NOISE))));
    let mut terms = d.shuffle(terms);
    if terms.is_empty() {
        terms.push("1 = 1".into());
    }
    let comment = d.pick(&["", " -- :c @d", " /* $e */"]);
    (
        format!("SELECT id FROM users WHERE {}{comment}", terms.join(" OR ")),
        names.into_iter().map(String::from).collect(),
    )
}

/// Whether SQLite runs `sql` with a value for each of `keys`, as Python's `sqlite3` binds a
/// mapping: each placeholder by its name without the sigil.
fn binds(sql: &str, keys: &Names) -> bool {
    let conn = rusqlite::Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(SCHEMA).expect("the schema");
    let mut statement = conn.prepare(sql).expect("the query prepares");
    let bound = (1..=statement.parameter_count()).all(|index| {
        statement
            .parameter_name(index)
            .is_some_and(|name| keys.contains(&name[1..]))
            && statement.raw_bind_parameter(index, 1).is_ok()
    });
    let ran = bound && statement.raw_query().next().is_ok();
    ran
}

#[test]
fn sqlite_names_match_the_driver() {
    check(named_query, |(sql, names)| {
        let params =
            engine::analyze_params(&sql, "sqlite", &SQLITE_SCHEMA).expect("the query parses");
        let found: Names = params.into_iter().filter_map(|param| param.name).collect();
        prop_assert_eq!(&found, &names);
        prop_assert!(binds(&sql, &found));
        for name in &found {
            let mut fewer = found.clone();
            fewer.remove(name);
            prop_assert!(!binds(&sql, &fewer), "binds without {}", name);
        }
        Ok(())
    });
}

static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"'(\w+)'").expect("a regex"));

#[test]
fn call_diagnostics_match_the_driver() {
    let case = |d: &mut Draw<'_>| {
        (
            named_query(d),
            d.subset(&NAMES, 0, 5)
                .into_iter()
                .map(String::from)
                .collect::<Names>(),
        )
    };
    check(case, |((sql, names), keys)| {
        let mapping = keys
            .iter()
            .map(|key| format!("'{key}': 1"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            "import sqlite3\n\nconn = sqlite3.connect('db')\nconn.execute({}, {{{mapping}}})\n",
            python::literal(&sql)
        );
        let found = engine::check(&source, "sqlite", &SQLITE_SCHEMA, None);
        let message = |rule: &str| {
            found
                .iter()
                .find(|(name, _)| *name == rule)
                .map(|(_, message)| message.as_str())
        };
        let missing: Names = names.difference(&keys).cloned().collect();
        let extra: Names = keys.difference(&names).cloned().collect();
        prop_assert_eq!(
            message("param-count-mismatch").is_some(),
            !missing.is_empty()
        );
        prop_assert_eq!(missing.is_empty(), binds(&sql, &keys));
        prop_assert_eq!(message("extra-param").is_some(), !extra.is_empty());
        for (rule, wanted) in [("param-count-mismatch", missing), ("extra-param", extra)] {
            if let Some(message) = message(rule) {
                let quoted: Names = QUOTED
                    .captures_iter(message)
                    .map(|found| found[1].to_string())
                    .collect();
                prop_assert_eq!(quoted, wanted, "{}", message);
            }
        }
        Ok(())
    });
}

const LINES: [&str; 4] = [
    "import os",
    "from sqlty import Bound",
    "from sqlty.drivers.psycopg import execute",
    "# NoParams",
];
const HEADERS: [&str; 6] = [
    "\"\"\"Doc.\"\"\"",
    "from __future__ import annotations",
    LINES[0],
    LINES[1],
    LINES[2],
    LINES[3],
];
const QUERIES: [&str; 4] = [
    "SELECT name FROM users",
    "SELECT id FROM users WHERE id = %s",
    "SELECT id FROM users WHERE id = %(id)s",
    "SELECT count(*) FROM users",
];
const STYLES: [Option<&str>; 3] = [
    None,
    Some("import sqlty"),
    Some("from sqlty import SQL as Q"),
];

/// `rewrite` with parameter types writes code that runs, and `strip` undoes it.
#[test]
fn param_types_rewrite_runs_and_strips() {
    let case = |d: &mut Draw<'_>| {
        let mut above = d.subset(&HEADERS, 0, 4);
        above.sort_by_key(|line| (!line.starts_with("\"\"\""), !line.contains("__future__")));
        let queries = d.list(1, 4, |d| d.pick(&QUERIES));
        let below = d.subset(&LINES, 0, 3);
        let assignments = queries
            .iter()
            .enumerate()
            .map(|(i, sql)| format!("q{i} = {}", python::literal(&format!("--sql\n{sql}"))));
        let lines: Vec<String> = above
            .iter()
            .map(ToString::to_string)
            .chain(assignments)
            .chain(below.iter().map(ToString::to_string))
            .collect();
        (lines.join("\n") + "\n", d.pick(&STYLES))
    };
    check(case, |(source, style)| {
        let rewritten = engine::rewrite(&source, &POSTGRES_SCHEMA, style);
        prop_assert_eq!(
            &engine::rewrite(&rewritten, &POSTGRES_SCHEMA, style),
            &rewritten
        );
        if let Err(error) = python::run(&rewritten) {
            return Err(TestCaseError::fail(format!("{error}\n{rewritten}")));
        }
        let found = engine::check(&rewritten, "postgres", &POSTGRES_SCHEMA, style);
        prop_assert!(
            !found
                .iter()
                .any(|(rule, _)| *rule == "params-type-mismatch"),
            "{}",
            rewritten
        );
        if !source.contains("# NoParams") {
            prop_assert_eq!(engine::strip(&rewritten, style), source);
        }
        Ok(())
    });
}
