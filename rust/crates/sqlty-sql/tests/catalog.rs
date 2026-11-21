//! What a loaded schema holds, read through `analyze_select` and `load_schema_with_skipped`:
//! the columns of tables and views after the statements that change them, in any order.

use sqlty_sql::analyzer::{
    analyze_select, analyze_with_diagnostics, load_schema_with_skipped, Column, Nullability, Schema,
};
use sqlty_sql::diagnostics::Rule;

fn load(sql: &str, dialect: &str) -> Schema {
    load_schema_with_skipped(sql, dialect).0
}

fn pg(sql: &str) -> Schema {
    load(sql, "postgres")
}

/// What `SELECT * FROM name` returns; `None` when the schema leaves the relation's shape unknown.
fn columns(schema: &Schema, dialect: &str, name: &str) -> Option<Vec<Column>> {
    let found = analyze_select(&format!("SELECT * FROM {name}"), dialect, Some(schema))?;
    match found.as_slice() {
        [star] if star.name == "*" => None,
        _ => Some(found),
    }
}

fn table(schema: &Schema, name: &str) -> Option<Vec<Column>> {
    columns(schema, "postgres", name)
}

/// `name:SQL_TYPE` of each column of `name`.
fn cols(schema: &Schema, name: &str) -> Vec<String> {
    spelled(&table(schema, name).unwrap_or_default())
}

fn spelled(columns: &[Column]) -> Vec<String> {
    columns
        .iter()
        .map(|c| format!("{}:{}", c.name, c.sql_type.as_deref().unwrap_or("?")))
        .collect()
}

fn not_null(columns: &[Column]) -> Vec<bool> {
    columns
        .iter()
        .map(|c| c.nullability == Nullability::NonNull)
        .collect()
}

/// Whether the schema defines `name`, however little it knows of its shape.
fn defines(schema: &Schema, name: &str) -> bool {
    let (_, diagnostics) =
        analyze_with_diagnostics(&format!("SELECT * FROM {name}"), "postgres", Some(schema));
    diagnostics.iter().all(|d| d.rule != Rule::UnknownTable)
}

#[test]
fn module_example_alter_table_adds_a_column() {
    let c = pg("CREATE TABLE users (id int PRIMARY KEY, name text); \
         ALTER TABLE users ADD COLUMN age int NOT NULL;");
    let columns = table(&c, "users").unwrap();
    assert_eq!(columns.len(), 3);
    assert_eq!(not_null(&columns), [true, false, true]);
}

#[test]
fn a_column_is_found_by_name_in_a_wide_table() {
    let columns: Vec<String> = (0..300).rev().map(|i| format!("c{i} int")).collect();
    let c = pg(&format!(
        "CREATE TABLE t ({}); CREATE VIEW v AS SELECT 1 AS x;",
        columns.join(", ")
    ));
    for i in 0..300 {
        let found = analyze_select(&format!("SELECT c{i} FROM t"), "postgres", Some(&c)).unwrap();
        assert_eq!(found[0].name, format!("c{i}"));
        assert_eq!(found[0].sql_type.as_deref(), Some("INT"));
    }
    let unknown_column = |sql: &str| {
        let (_, diagnostics) = analyze_with_diagnostics(sql, "postgres", Some(&c));
        diagnostics.iter().any(|d| d.rule == Rule::UnknownColumn)
    };
    assert!(unknown_column("SELECT c300 FROM t"));
    assert!(unknown_column("SELECT \"\" FROM t"));
    assert!(analyze_select("SELECT x FROM v", "postgres", Some(&c)).is_some());
    let (_, diagnostics) = analyze_with_diagnostics("SELECT c1 FROM nope", "postgres", Some(&c));
    assert!(diagnostics.iter().any(|d| d.rule == Rule::UnknownTable));
}

#[test]
fn alter_table_changes_columns() {
    let c = pg("CREATE TABLE t (a int, b text, c int);
        ALTER TABLE t ADD COLUMN d bool NOT NULL;
        ALTER TABLE t DROP COLUMN b;
        ALTER TABLE t RENAME COLUMN c TO cc;
        ALTER TABLE t ALTER COLUMN a TYPE bigint;
        ALTER TABLE t ALTER COLUMN a SET NOT NULL;");
    assert_eq!(cols(&c, "t"), ["a:BIGINT", "cc:INT", "d:BOOLEAN"]);
    assert_eq!(not_null(&table(&c, "t").unwrap()), [true, false, true]);
}

#[test]
fn statements_may_come_in_any_order() {
    let c = pg("ALTER TABLE t ADD COLUMN b text;
        CREATE VIEW v AS SELECT a, b FROM t;
        CREATE TABLE t (a int);");
    assert_eq!(cols(&c, "t"), ["a:INT", "b:TEXT"]);
    assert_eq!(table(&c, "v").unwrap().len(), 2);
}

#[test]
fn search_path_and_qualified_names() {
    let c = pg("CREATE SCHEMA app;
        SET search_path TO app, public;
        CREATE TABLE users (id int);
        CREATE TABLE public.users (id text);");
    assert_eq!(cols(&c, "users"), ["id:INT"]);
    assert_eq!(cols(&c, "public.users"), ["id:TEXT"]);
    assert_eq!(cols(&c, "app.users"), ["id:INT"]);
}

#[test]
fn an_ambiguous_bare_name_is_unknown() {
    let c = pg("CREATE TABLE a.t (x int); CREATE TABLE b.t (x text);");
    assert!(table(&c, "t").is_none());
    assert_eq!(cols(&c, "a.t"), ["x:INT"]);
}

#[test]
fn inherits_partition_and_like_copy_columns() {
    let c = pg("CREATE TABLE child (extra int) INHERITS (base);
        CREATE TABLE base (id int NOT NULL);
        CREATE TABLE part PARTITION OF parent FOR VALUES FROM (1) TO (10);
        CREATE TABLE parent (k int) PARTITION BY RANGE (k);
        CREATE TABLE copy (LIKE base INCLUDING ALL, more text);
        ALTER TABLE parent ADD COLUMN v text;");
    assert_eq!(cols(&c, "child"), ["id:INT", "extra:INT"]);
    assert_eq!(cols(&c, "part"), ["k:INT", "v:TEXT"]);
    assert_eq!(cols(&c, "copy"), ["id:INT", "more:TEXT"]);
    assert!(not_null(&table(&c, "copy").unwrap())[0]);
}

#[test]
fn a_renamed_parent_still_alters_its_children() {
    let c = pg("CREATE TABLE base (id int);
        CREATE TABLE child (extra int) INHERITS (base);
        ALTER TABLE base RENAME TO root;
        ALTER TABLE root ADD COLUMN v text;");
    assert_eq!(cols(&c, "child"), ["id:INT", "extra:INT", "v:TEXT"]);
}

#[test]
fn a_table_renamed_over_another_replaces_it() {
    let c = pg("CREATE TABLE a (x int); CREATE TABLE b (y int); ALTER TABLE a RENAME TO b;");
    assert_eq!(cols(&c, "b"), ["x:INT"]);
    assert!(!defines(&c, "a"));
}

#[test]
fn an_unknown_like_source_leaves_an_unknown_shape() {
    let c = pg("CREATE TABLE copy (LIKE missing);");
    assert!(table(&c, "copy").is_none());
    assert!(defines(&c, "copy"));
}

#[test]
fn an_unparsable_alter_leaves_an_unknown_shape() {
    let c = pg("CREATE TABLE t (a int);
        ALTER TABLE t SET SCHEMA other;
        CREATE TABLE u (a int);
        ALTER TABLE u OWNER TO me;");
    assert!(table(&c, "t").is_none());
    assert_eq!(cols(&c, "u"), ["a:INT"]);
}

#[test]
fn an_enum_altered_before_it_is_created_keeps_its_values() {
    let c = pg("CREATE TABLE t (m mood);
        ALTER TYPE mood ADD VALUE 'meh' BEFORE 'sad';
        CREATE TYPE mood AS ENUM ('happy', 'sad');");
    let column = &table(&c, "t").unwrap()[0];
    assert_eq!(
        column.python_type.as_deref(),
        Some("typing.Literal['happy', 'meh', 'sad']")
    );
}

#[test]
fn drop_removes_tables_views_and_types() {
    let c = pg(
        "CREATE TABLE a (x int); CREATE TABLE b (x int); CREATE VIEW v AS SELECT x FROM b;
        DROP TABLE a; DROP VIEW v; DROP TABLE IF EXISTS missing;
        CREATE TYPE e AS ENUM ('x'); DROP TYPE e;
        CREATE TABLE t (m e);",
    );
    assert!(!defines(&c, "a") && !defines(&c, "v"));
    assert!(table(&c, "b").is_some());
    assert_eq!(table(&c, "t").unwrap()[0].python_type, None);
}

#[test]
fn a_table_created_after_its_drop_is_kept() {
    let c = pg("DROP TABLE t; CREATE TABLE t (a int);");
    assert_eq!(cols(&c, "t"), ["a:INT"]);
}

#[test]
fn functions_return_scalars_sets_and_rows() {
    let c = pg("CREATE FUNCTION one() RETURNS int LANGUAGE sql AS 'select 1';
        CREATE FUNCTION many(a int, b text DEFAULT 'x') RETURNS SETOF text LANGUAGE sql AS 'select 1';
        CREATE FUNCTION rows(a int) RETURNS TABLE(id int, name text) LANGUAGE sql AS 'select 1, 2';
        CREATE TABLE t (id int);
        CREATE FUNCTION of_t() RETURNS SETOF t LANGUAGE sql AS 'select 1';");
    let select = |sql: &str| analyze_select(sql, "postgres", Some(&c)).unwrap();
    assert_eq!(
        select("SELECT one()")[0].python_type.as_deref(),
        Some("int")
    );
    // `one` takes no argument.
    assert_eq!(select("SELECT one(1)")[0].python_type, None);
    assert_eq!(
        select("SELECT * FROM many(1)")[0].python_type.as_deref(),
        Some("str")
    );
    assert_eq!(select("SELECT * FROM rows(1)").len(), 2);
    assert_eq!(select("SELECT * FROM of_t()").len(), 1);
}

#[test]
fn a_redefined_function_with_an_unreadable_body_is_unknown() {
    let c = pg("CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'select 1';
        CREATE OR REPLACE FUNCTION f() RETURNS int LANGUAGE sql AS $$ select 1 $$ SECURITY NONSENSE;");
    assert_eq!(
        analyze_select("SELECT f()", "postgres", Some(&c)).unwrap()[0].python_type,
        None
    );
}

#[test]
fn builtin_relations_are_always_there() {
    let c = pg("");
    assert!(table(&c, "pg_class").is_some());
    assert!(table(&c, "pg_catalog.pg_namespace").is_some());
    assert!(table(&c, "information_schema.columns").is_some());
    assert!(table(&c, "columns").is_none());
    assert!(c.is_empty());
}

#[test]
fn primary_keys_and_serials_are_not_null() {
    let c = pg("CREATE TABLE t (a int PRIMARY KEY, b serial, c int, d int, PRIMARY KEY (a, d));");
    assert_eq!(
        not_null(&table(&c, "t").unwrap()),
        [true, true, false, true]
    );
}

#[test]
fn other_dialects_ignore_postgres_statements() {
    let c = load(
        "CREATE TABLE db.t (a INT); ALTER TABLE t ADD COLUMN b INT; DROP TABLE t;",
        "duckdb",
    );
    assert_eq!(columns(&c, "duckdb", "t").map(|c| c.len()), Some(1));
    assert!(columns(&c, "duckdb", "pg_class").is_none());
}

#[test]
fn mysql_and_sqlite_follow_alter_and_drop() {
    for dialect in ["mysql", "sqlite"] {
        let c = load(
            "CREATE TABLE t (a INT); ALTER TABLE t ADD COLUMN b INT; \
             CREATE TABLE u (a INT); DROP TABLE u;",
            dialect,
        );
        assert_eq!(
            columns(&c, dialect, "t").map(|c| c.len()),
            Some(2),
            "{dialect}"
        );
        assert!(columns(&c, dialect, "u").is_none(), "{dialect}");
    }
}

/// Catches loading that grows quadratically, which takes far longer at these sizes; a
/// debug build on a shared CI runner takes up to a second.
fn assert_fast(sql: &str) -> Schema {
    let start = std::time::Instant::now();
    let catalog = pg(sql);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "load took {:?}",
        start.elapsed()
    );
    catalog
}

#[test]
fn statements_that_never_resolve_load_fast() {
    let mut sql = String::new();
    for i in 0..2000 {
        sql += &format!("CREATE TABLE t{i} (id int);\n");
        sql += &format!("ALTER TABLE ext{i} ADD COLUMN x int;\n");
    }
    let catalog = assert_fast(&sql);
    assert_eq!(cols(&catalog, "t1999"), ["id:INT"]);
}

#[test]
fn many_views_over_altered_tables_load_fast() {
    let mut sql = String::new();
    for i in 0..2000 {
        sql += &format!("CREATE TABLE t{i} (id int);\n");
        sql += &format!("ALTER TABLE t{i} ADD COLUMN c int;\n");
        sql += &format!("CREATE VIEW v{i} AS SELECT * FROM t{i};\n");
    }
    let catalog = assert_fast(&sql);
    assert_eq!(table(&catalog, "v1999").unwrap().len(), 2);
}

#[test]
fn a_view_chain_loads_fast() {
    let mut sql = String::from("CREATE TABLE v0 (id int);\n");
    for i in 1..1500 {
        sql += &format!("CREATE VIEW v{i} AS SELECT id FROM v{};\n", i - 1);
    }
    let catalog = assert_fast(&sql);
    assert_eq!(table(&catalog, "v1499").unwrap().len(), 1);
}

#[test]
fn degraded_objects_are_reported() {
    let (c, skipped) = load_schema_with_skipped(
        "CREATE TABLE a (x int); ALTER TABLE a SET SCHEMA other;
         CREATE TABLE b (x int); ALTER TABLE b SET (fillfactor = 70);
         CREATE TABLE c (LIKE nothing);
         CREATE TYPE e AS ENUM ('x'); ALTER TYPE e SET SCHEMA other;",
        "postgres",
    );
    assert!(table(&c, "a").is_none() && table(&c, "b").is_some());
    let names: Vec<_> = skipped
        .iter()
        .map(|(kind, name)| format!("{kind} {name}"))
        .collect();
    assert_eq!(names, ["table a", "type e", "table c"]);
}

#[test]
fn if_not_exists_keeps_the_existing_relation() {
    let c = pg("CREATE TABLE t (a int);
        CREATE TABLE IF NOT EXISTS t (a text, b text);
        CREATE VIEW v AS SELECT a FROM t;
        CREATE VIEW IF NOT EXISTS v AS SELECT a, a AS b FROM t;
        CREATE TABLE IF NOT EXISTS fresh (c int);");
    assert_eq!(cols(&c, "t"), ["a:INT"]);
    assert_eq!(table(&c, "v").unwrap().len(), 1);
    assert_eq!(cols(&c, "fresh"), ["c:INT"]);
}

#[test]
fn a_renamed_enum_keeps_its_columns_literal() {
    let c = pg("CREATE TYPE mood AS ENUM ('a', 'b');
        CREATE TABLE t (m mood);
        ALTER TYPE mood RENAME TO feel;
        ALTER TYPE feel ADD VALUE 'c';
        CREATE TABLE u (f feel, m2 mood);");
    let t = table(&c, "t").unwrap();
    assert_eq!(t[0].sql_type.as_deref(), Some("FEEL"));
    assert_eq!(
        t[0].python_type.as_deref(),
        Some("typing.Literal['a', 'b', 'c']")
    );
    assert_eq!(table(&c, "u").unwrap()[0].sql_type.as_deref(), Some("FEEL"));
}

/// A view read first caches the catalog; the functions after it must not each rebuild
/// the function table. Time should grow about linearly with the count.
#[test]
#[ignore = "timing, run by hand in release mode"]
fn perf_view_then_many_functions() {
    for n in [500, 1000, 2000, 4000] {
        let mut sql =
            String::from("CREATE TABLE t (id integer);\nCREATE VIEW v AS SELECT id FROM t;\n");
        for i in 0..n {
            sql.push_str(&format!(
                "CREATE FUNCTION f{i}(a integer) RETURNS integer LANGUAGE sql AS 'SELECT 1';\n"
            ));
        }
        let start = std::time::Instant::now();
        let catalog = pg(&sql);
        let elapsed = start.elapsed();
        assert!(table(&catalog, "t").is_some());
        println!("perf view + {n:5} functions: {elapsed:>10.2?}");
    }
}

/// A view created after the catalog was cached sees what the statements before it
/// did to functions, exactly as if the catalog had been built from scratch.
#[test]
fn views_see_functions_changed_after_the_cache_was_built() {
    let c = pg("CREATE TABLE t (id int);
        CREATE VIEW v1 AS SELECT id FROM t;
        CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'select 1';
        CREATE VIEW v2 AS SELECT f() AS x;
        CREATE OR REPLACE FUNCTION f() RETURNS text LANGUAGE sql AS 'select 1';
        CREATE VIEW v3 AS SELECT f() AS x;
        DROP FUNCTION f();
        CREATE VIEW v4 AS SELECT f() AS x;");
    assert_eq!(spelled(&table(&c, "v2").unwrap()), ["x:INT"]);
    assert_eq!(spelled(&table(&c, "v3").unwrap()), ["x:TEXT"]);
    assert_eq!(spelled(&table(&c, "v4").unwrap()), ["x:?"]);
}

/// `SETOF table` functions follow later changes to the table without a rebuild.
#[test]
fn a_setof_table_function_follows_alter_after_the_cache_was_built() {
    let c = pg("CREATE TABLE t (id int);
        CREATE FUNCTION rows() RETURNS SETOF t LANGUAGE sql AS 'select 1';
        CREATE VIEW v1 AS SELECT * FROM rows();
        ALTER TABLE t ADD COLUMN name text;
        CREATE VIEW v2 AS SELECT * FROM rows();
        ALTER TABLE t OWNER TO app;
        CREATE VIEW v3 AS SELECT * FROM rows();");
    assert_eq!(cols(&c, "v1"), ["id:INT"]);
    assert_eq!(cols(&c, "v2"), ["id:INT", "name:TEXT"]);
    assert_eq!(cols(&c, "v3"), ["id:INT", "name:TEXT"]);
}

/// A function returning a table's row follows a later change to the table.
#[test]
fn a_function_returning_a_row_follows_its_table() {
    let c = pg("CREATE TABLE t (id int);
        CREATE VIEW v1 AS SELECT id FROM t;
        CREATE FUNCTION f() RETURNS t LANGUAGE sql AS 'select 1';
        ALTER TABLE t ADD COLUMN name text;
        CREATE VIEW v2 AS SELECT * FROM f();");
    assert_eq!(cols(&c, "v2"), ["id:INT", "name:TEXT"]);
}

#[test]
fn a_function_changed_before_a_rebuild_resolves_with_it() {
    let c = pg("CREATE TABLE t (id int);
        CREATE VIEW v1 AS SELECT id FROM t;
        CREATE FUNCTION f() RETURNS SETOF u LANGUAGE sql AS 'select 1';
        CREATE TABLE u (name text);
        CREATE VIEW v2 AS SELECT * FROM f();");
    assert_eq!(cols(&c, "v2"), ["name:TEXT"]);
}

#[test]
fn a_function_changed_before_a_type_is_resolved_in_the_rebuild() {
    let c = pg("CREATE TABLE t (id int);
        CREATE VIEW v1 AS SELECT id FROM t;
        CREATE FUNCTION f() RETURNS int LANGUAGE sql AS 'select 1';
        CREATE TYPE e AS ENUM ('a');
        CREATE VIEW v2 AS SELECT f() AS n;");
    assert_eq!(cols(&c, "v2"), ["n:INT"]);
}

#[test]
fn a_view_waiting_with_a_table_is_read_after_it() {
    let c = pg("CREATE TABLE a (LIKE nope); CREATE VIEW v AS SELECT 1 AS x FROM nope2;");
    assert_eq!(cols(&c, "v"), ["x:?"]);
}
