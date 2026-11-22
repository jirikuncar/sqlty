//! Statements the loader reads from their tokens when sqlparser does not parse them, seen in
//! what a schema defines afterwards.

use sqlty_sql::analyzer::{
    analyze_select, analyze_with_diagnostics, load_schema_with_skipped, Nullability,
};

/// The skipped objects of `ddl`, and `(name, SQL type)` of each column `sql` returns after it.
fn read(ddl: &str, dialect: &str, sql: &str) -> (Vec<(&'static str, String)>, Vec<String>) {
    let (schema, skipped) = load_schema_with_skipped(ddl, dialect);
    let found = analyze_select(sql, dialect, Some(&schema)).unwrap_or_default();
    let columns = found
        .into_iter()
        .map(|c| format!("{}:{}", c.name, c.sql_type.unwrap_or_default()))
        .collect();
    (skipped, columns)
}

#[test]
fn an_alter_names_its_target_through_quotes_if_exists_and_only() {
    let table = "CREATE SCHEMA app; CREATE TABLE app.\"Users\" (id int);";
    let select = "SELECT * FROM app.\"Users\"";
    // `OWNER TO` changes no column.
    let owner = format!("{table} ALTER TABLE IF EXISTS ONLY app.\"Users\" OWNER TO me;");
    assert_eq!(
        read(&owner, "postgres", select),
        (vec![], vec!["id:INT".to_string()])
    );
    // What sqlparser rejects and could change a column leaves the named table unknown.
    let broken = format!(
        "{table} ALTER TABLE IF EXISTS ONLY app.\"Users\" ALTER COLUMN id TYPE int USING (id +;"
    );
    let (skipped, columns) = read(&broken, "postgres", select);
    assert_eq!(skipped, [("table", "app.Users".to_string())]);
    assert_eq!(columns, ["*:"]);
}

#[test]
fn drop_materialized_view_drops_the_view() {
    let create = "CREATE MATERIALIZED VIEW v AS SELECT 1 AS x;";
    assert_eq!(read(create, "postgres", "SELECT * FROM v").1, ["x:"]);
    let dropped = format!("{create} DROP MATERIALIZED VIEW IF EXISTS v CASCADE;");
    assert_eq!(read(&dropped, "postgres", "SELECT * FROM v").1, ["*:"]);
}

#[test]
fn a_partition_is_a_table_and_a_view_is_not() {
    let partition = "CREATE TABLE q (a int); CREATE TABLE p PARTITION OF q DEFAULT;";
    assert_eq!(read(partition, "postgres", "SELECT * FROM p").1, ["a:INT"]);
    // `ALTER TABLE` does not follow a view as it follows a table.
    let view = "CREATE VIEW v AS SELECT 1 AS x; ALTER TABLE v ADD COLUMN y int;";
    assert_eq!(
        read(view, "postgres", "SELECT * FROM v").0,
        [("table", "v".to_string())]
    );
}

#[test]
fn a_virtual_table_declares_the_columns_of_its_module() {
    let (skipped, columns) = read(
        "CREATE VIRTUAL TABLE v USING fts5(a(b), c);",
        "sqlite",
        "SELECT * FROM v",
    );
    assert!(skipped.is_empty());
    assert_eq!(columns, ["a:", "c:"]);
    let (skipped, _) = read(
        "CREATE VIRTUAL TABLE v USING m(a(b), c);",
        "sqlite",
        "SELECT * FROM v",
    );
    assert_eq!(skipped, [("table", "v".to_string())]);
    let rtree = "CREATE VIRTUAL TABLE IF NOT EXISTS v USING rtree(id, x0, x1, +name);";
    assert_eq!(
        read(rtree, "sqlite", "SELECT * FROM v").1,
        ["id:INT", "x0:FLOAT", "x1:FLOAT", "name:"]
    );
}

#[test]
fn a_before_row_trigger_may_change_the_row_returned() {
    let table = "CREATE TABLE t (a int);";
    let nullability = |ddl: &str| {
        let (schema, _) = load_schema_with_skipped(ddl, "postgres");
        let sql = "INSERT INTO t (a) VALUES (1) RETURNING a";
        let (found, _) = analyze_with_diagnostics(sql, "postgres", Some(&schema));
        found.unwrap_or_default()[0].nullability
    };
    assert_eq!(nullability(table), Nullability::NonNull);
    let trigger = "CREATE TRIGGER IF NOT EXISTS x BEFORE INSERT ON t FOR EACH ROW BEGIN END;";
    assert_eq!(
        nullability(&format!("{table} {trigger}")),
        Nullability::Nullable
    );
    let after = "CREATE TRIGGER x AFTER INSERT ON t FOR EACH ROW EXECUTE FUNCTION f();";
    assert_eq!(
        nullability(&format!("{table} {after}")),
        Nullability::NonNull
    );
}

#[test]
fn a_function_that_is_not_read_taints_its_qualified_name() {
    let (skipped, _) = read(
        "CREATE TYPE x; CREATE FUNCTION IF NOT EXISTS s.f() RETURNS int LANGUAGE sql AS 'select 1' STRICT;",
        "postgres",
        "SELECT s.f()",
    );
    assert_eq!(skipped, [("function", "s.f".to_string())]);
    let (skipped, _) = read(
        "CREATE OR REPLACE FUNCTION f( AS ;",
        "postgres",
        "SELECT f()",
    );
    assert_eq!(skipped, [("function", "f".to_string())]);
}
