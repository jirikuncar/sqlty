//! What a schema leaves unknown past the loader's size limits, read from `RelationKind` and
//! the skipped objects `load_schema_with_skipped` reports.

#![expect(
    clippy::unwrap_used,
    reason = "a test fails on the query it cannot read"
)]

use sqlty_sql::analyzer::{load_schema_with_skipped, Columns, RelationKind, Schema};

// The loader's limits, which no public item names: a change to one fails these tests.
/// Columns one table may hold (`MAX_COLUMNS`).
const MAX_COLUMNS: usize = 4096;
/// Columns all relations of a schema may hold (`MAX_SCHEMA_COLUMNS`), besides its allowance.
const MAX_SCHEMA_COLUMNS: usize = 1 << 21;
/// Statements the loader retries (`MAX_RETRIED`).
const MAX_RETRIED: usize = 1 << 24;
/// Columns the loader writes (`MAX_WRITTEN`).
const MAX_WRITTEN: usize = 1 << 22;

type Skipped = Vec<(&'static str, String)>;

fn load(sql: &str, dialect: &str) -> (Schema, Skipped) {
    load_schema_with_skipped(sql, dialect)
}

fn wide(name: &str, width: usize) -> String {
    let columns: Vec<String> = (0..width).map(|i| format!("c{i} int")).collect();
    format!("CREATE TABLE {name} ({});\n", columns.join(", "))
}

/// How many columns the table `name` holds, `None` when it is defined with an unknown shape,
/// and `None` for a name the schema does not define.
fn width(schema: &Schema, name: &str) -> Option<usize> {
    match schema.columns_of(&format!("public.{name}"))? {
        Columns::Table(columns) => Some(columns.len()),
        Columns::View(columns) => Some(columns.len()),
    }
}

/// Whether the schema defines `name` at all, with whatever shape.
fn defines(schema: &Schema, name: &str) -> bool {
    schema
        .relations_with_prefix(name, None, 100)
        .iter()
        .any(|found| found.name.name == name)
}

fn kind(schema: &Schema, name: &str) -> Option<RelationKind> {
    schema
        .relations_with_prefix(name, None, 100)
        .iter()
        .find(|found| found.name.name == name)
        .map(|found| found.kind)
}

fn cols(schema: &Schema, name: &str) -> Vec<String> {
    match schema.columns_of(&format!("public.{name}")) {
        Some(Columns::Table(columns)) => columns
            .iter()
            .map(|c| format!("{}:{}", c.name, c.sql_type))
            .collect(),
        _ => Vec::new(),
    }
}

#[test]
fn a_table_wider_than_max_columns_is_unknown() {
    let sql = wide("ok", MAX_COLUMNS) + &wide("wide", MAX_COLUMNS + 1);
    let (c, skipped) = load(&sql, "postgres");
    assert_eq!(width(&c, "ok"), Some(MAX_COLUMNS));
    assert!(width(&c, "wide").is_none() && defines(&c, "wide"));
    assert_eq!(skipped, [("limit", "public.wide".to_string())]);
}

#[test]
fn relations_past_the_schema_columns_are_unknown() {
    // The built-in relations hold some columns too: the last copies may not fit.
    // The schema's own text allows fewer than 16 more.
    let copies = MAX_SCHEMA_COLUMNS / MAX_COLUMNS + 16;
    let mut sql = wide("big", MAX_COLUMNS) + "CREATE SCHEMA s;\n";
    for i in 0..copies {
        sql += &format!("CREATE TABLE s.t{i} (LIKE big);\n");
    }
    sql += "CREATE TABLE copy (LIKE big);\nCREATE TABLE child () INHERITS (big);\n";
    sql += &wide("one", MAX_COLUMNS);
    sql += "CREATE VIEW v AS SELECT * FROM big;
        DROP SCHEMA s CASCADE;
        CREATE TABLE two (a int);
        CREATE TABLE copy2 (LIKE big);";
    let (c, skipped) = load(&sql, "postgres");
    for name in ["copy", "child", "one"] {
        assert!(width(&c, name).is_none() && defines(&c, name), "{name}");
    }
    assert_eq!(kind(&c, "v"), Some(RelationKind::View));
    assert!(
        matches!(c.columns_of("v"), Some(Columns::View(columns)) if columns.len() == 1 && columns[0].name == "*")
    );
    assert_eq!(cols(&c, "two"), ["a:INT"]);
    assert_eq!(width(&c, "copy2"), Some(MAX_COLUMNS));
    let (copied, rest) = skipped.split_at(skipped.len() - 4);
    assert!(copied.iter().all(|(_, name)| name.starts_with("s.t")));
    assert_eq!(
        rest,
        [
            ("limit", "copy".to_string()),
            ("limit", "child".to_string()),
            ("limit", "public.one".to_string()),
            ("limit", "public.v".to_string()),
        ]
    );
}

#[test]
fn a_long_schema_may_hold_as_many_columns_as_it_spells_out() {
    let copies = MAX_SCHEMA_COLUMNS / MAX_COLUMNS + 16;
    let mut sql = wide("big", MAX_COLUMNS);
    for i in 0..copies {
        sql += &format!("CREATE TABLE t{i} (LIKE big);\n");
    }
    let (c, _) = load(&sql, "postgres");
    assert!(width(&c, &format!("t{}", copies - 1)).is_none());
    sql += &" ".repeat(2 * MAX_SCHEMA_COLUMNS);
    let (c, skipped) = load(&sql, "postgres");
    assert!(width(&c, &format!("t{}", copies - 1)).is_some() && skipped.is_empty());
}

#[test]
fn a_view_past_the_limits_counts_as_it_is_held() {
    let copies = MAX_SCHEMA_COLUMNS / MAX_COLUMNS + 32;
    let mut sql = wide("big", MAX_COLUMNS);
    for i in 0..copies {
        sql += &format!("CREATE VIEW f{i} AS SELECT * FROM big;\n");
    }
    sql += "CREATE VIEW limited AS SELECT * FROM big;\n";
    for i in 0..copies {
        sql += &format!("DROP VIEW f{i};\n");
    }
    // An unknown table holds no columns: renamed over the view, it frees the view's one.
    sql += "DROP TABLE big;\n";
    sql += &wide("e", MAX_COLUMNS + 1);
    sql += "ALTER TABLE e RENAME TO limited;";
    let (c, skipped) = load(&sql, "sqlite");
    assert!(skipped.contains(&("limit", "public.limited".to_string())));
    assert!(width(&c, "limited").is_none() && defines(&c, "limited"));
}

#[test]
fn a_chain_written_backwards_stops_after_max_retried() {
    // Each pass creates one more table: n passes try about n * n / 2 statements of
    // some 30 bytes.
    let n = 2 * (MAX_RETRIED as f64 / 16.0).sqrt() as usize;
    let mut sql: String = (0..n)
        .map(|i| format!("CREATE TABLE t{i} (LIKE t{});\n", i + 1))
        .collect();
    sql += &format!("CREATE TABLE t{n} (a int);");
    let (c, skipped) = load(&sql, "postgres");
    assert_eq!(cols(&c, &format!("t{}", n - 1)), ["a:INT"]);
    assert!(width(&c, "t0").is_none() && defines(&c, "t0"));
    assert!(skipped.contains(&("table", "t0".to_string())));
}

fn indexes(n: usize) -> String {
    (0..n)
        .map(|i| format!("CREATE UNIQUE INDEX i{i} ON big (c{});\n", i % MAX_COLUMNS))
        .collect()
}

/// The fewest writes of a [`MAX_COLUMNS`] wide table that pass [`MAX_WRITTEN`] and the
/// allowance of the schema `sql` gives for that many.
fn writes_past_the_limit(sql: impl Fn(usize) -> String) -> (usize, String) {
    (MAX_WRITTEN / MAX_COLUMNS..)
        .map(|n| (n, sql(n)))
        .find(|(n, sql)| n * MAX_COLUMNS > MAX_WRITTEN + sql.len() / 2)
        .unwrap()
}

#[test]
fn loading_stops_after_max_written_columns() {
    let (_, sql) = writes_past_the_limit(|n| {
        "CREATE TABLE waits (LIKE later);\n".to_string()
            + &wide("big", MAX_COLUMNS)
            + &indexes(n - 1)
            + "CREATE TABLE later (a int);\nCREATE TABLE ignored (a int);"
    });
    let (c, skipped) = load(&sql, "postgres");
    assert!(!defines(&c, "later") && !defines(&c, "waits"));
    assert_eq!(
        skipped,
        [
            ("limit", "CREATE TABLE later (a int) and later".to_string()),
            ("limit", "1 statements waiting on others".to_string()),
        ]
    );
    // With nothing waiting, only what is left unread is named.
    let sql = sql.replacen("CREATE TABLE waits (LIKE later);\n", "", 1);
    assert_eq!(
        load(&sql, "postgres").1,
        [("limit", "CREATE TABLE later (a int) and later".to_string())]
    );
}

#[test]
fn statements_waiting_on_others_stop_after_max_written_columns() {
    // Defined after the indexes on it, so they wait for it.
    let (_, sql) = writes_past_the_limit(|n| indexes(n) + &wide("big", MAX_COLUMNS));
    let (c, skipped) = load(&sql, "postgres");
    assert_eq!(width(&c, "big"), Some(MAX_COLUMNS));
    assert_eq!(
        skipped,
        [("limit", "1 statements waiting on others".to_string())]
    );
    // Views that read a table never defined are read last, against what is there.
    let (_, sql) = writes_past_the_limit(|n| {
        wide("big", MAX_COLUMNS)
            + &indexes(n - 2)
            + "CREATE VIEW v1 AS SELECT big.* FROM big, missing;
               CREATE VIEW v2 AS SELECT big.* FROM big, missing;"
    });
    let (c, skipped) = load(&sql, "postgres");
    assert!(defines(&c, "v1") && !defines(&c, "v2"));
    assert_eq!(
        skipped,
        [("limit", "1 statements waiting on others".to_string())]
    );
}
