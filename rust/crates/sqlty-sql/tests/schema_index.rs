//! What a loaded schema lists by prefix: relations, functions, types, the tables a foreign key
//! links, and the columns of a relation, through the methods of `SchemaData`.

use sqlty_sql::analyzer::{
    analyze_with_diagnostics, load_schema_with_skipped, Columns, RelationRef, Schema,
};
use sqlty_sql::diagnostics::Rule;

fn load(sql: &str, dialect: &str) -> Schema {
    load_schema_with_skipped(sql, dialect).0
}

fn pg(sql: &str) -> Schema {
    load(sql, "postgres")
}

fn names(found: &[RelationRef<'_>]) -> Vec<String> {
    found
        .iter()
        .map(|r| match r.name.schema {
            Some(schema) => format!("{schema}.{}:{:?}", r.name.name, r.kind),
            None => format!("{}:{:?}", r.name.name, r.kind),
        })
        .collect()
}

#[test]
fn visible_relations_list_the_path_first() {
    let c = pg("CREATE SCHEMA app; SET search_path TO app, public;
        CREATE TABLE users (id int); CREATE TABLE public.users (id text);
        CREATE TABLE public.pg_users (id int); CREATE TABLE public.user_roles (id int);
        CREATE TABLE public.pg_class (id int); CREATE TABLE other.user_secrets (id int);");
    let found = c.visible_relations_with_prefix("", 4);
    assert_eq!(
        names(&found)[..3],
        [
            "app.users:Table",
            "public.pg_users:Table",
            "public.user_roles:Table"
        ]
    );
    assert_eq!(names(&found)[3], "pg_catalog.pg_attrdef:Table");
    let found = c.visible_relations_with_prefix("pg_cla", 10);
    assert_eq!(names(&found), ["pg_catalog.pg_class:Table"]);
    let found = c.visible_relations_with_prefix("USER", 10);
    assert_eq!(
        names(&found),
        ["app.users:Table", "public.user_roles:Table"]
    );
}

#[test]
fn schema_narrows_to_one_schema() {
    let c = pg(
        "CREATE TABLE users (id int); CREATE TABLE other.users (id int);
        CREATE TABLE other.user_secrets (id int); CREATE TABLE \"Other\".users (id int);",
    );
    let found = c.relations_with_prefix("us", Some("other"), 10);
    assert_eq!(
        names(&found),
        ["other.user_secrets:Table", "other.users:Table"]
    );
    let found = c.relations_with_prefix("", Some("OTHER"), 10);
    assert_eq!(found.len(), 2);
    let found = c.relations_with_prefix("", Some("\"Other\""), 10);
    assert_eq!(names(&found), ["Other.users:Table"]);
    let found = c.relations_with_prefix("col", Some("information_schema"), 10);
    assert_eq!(names(&found), ["information_schema.columns:Table"]);
    assert!(c.relations_with_prefix("col", None, 10).is_empty());
}

#[test]
fn views_and_unknown_shapes() {
    let c = pg(
        "CREATE TABLE ta (id int, name text); CREATE VIEW tv AS SELECT id FROM ta;
        CREATE TABLE tu (LIKE missing);",
    );
    let found = c.relations_with_prefix("t", None, 10);
    assert_eq!(
        names(&found),
        ["public.ta:Table", "public.tu:Unknown", "public.tv:View"]
    );
    assert!(
        matches!(c.columns_of(found[0].name.key), Some(Columns::Table(cols)) if cols.len() == 2)
    );
    assert!(matches!(c.columns_of("tv"), Some(Columns::View(cols)) if cols.len() == 1));
    assert!(c.columns_of(found[1].name.key).is_none());
    assert!(c.columns_of("missing").is_none());
}

#[test]
fn functions() {
    let c = pg(
        "CREATE FUNCTION total() RETURNS int LANGUAGE sql AS 'select 1';
        CREATE FUNCTION tally() LANGUAGE sql AS 'select 1';
        CREATE TYPE tone AS ENUM ('a'); CREATE TYPE tpoint AS (x int);
        CREATE DOMAIN tpct AS int;",
    );
    let functions: Vec<_> = c
        .functions_with_prefix("t", Some("public"), 10)
        .iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(functions, ["tally", "total"]);
    assert_eq!(
        c.functions_with_prefix("tot", None, 10)[0].key,
        "public.total"
    );
}

/// `(table, referred -> column pairs)` of each join of `name` with `prefix`.
fn joins(c: &Schema, name: &str, prefix: &str) -> Vec<(String, Vec<(String, String)>)> {
    c.joins_with_prefix(name, prefix, 5)
        .iter()
        .map(|j| {
            let pairs = j.pairs.iter().map(|(a, b)| (a.to_string(), b.to_string()));
            (j.table.name.to_string(), pairs.collect())
        })
        .collect()
}

#[test]
fn foreign_keys_reach_tables_defined_later() {
    let c = pg("SET search_path = app; \
         CREATE TABLE a (b_id int REFERENCES b, c_id int REFERENCES c (code), gone int REFERENCES gone); \
         CREATE TABLE app.b (id int PRIMARY KEY); \
         CREATE TABLE app.c (code text UNIQUE);");
    let pair = |a: &str, b: &str| vec![(a.to_string(), b.to_string())];
    assert_eq!(
        joins(&c, "app.a", ""),
        [
            ("b".to_string(), pair("id", "b_id")),
            ("c".to_string(), pair("code", "c_id"))
        ]
    );
}

#[test]
fn a_dropped_named_key_leaves_the_others() {
    let c = pg("CREATE TABLE p (id int PRIMARY KEY);
        CREATE TABLE c (pid int,
            CONSTRAINT k FOREIGN KEY (pid) REFERENCES p (id),
            CONSTRAINT k2 FOREIGN KEY (pid) REFERENCES p (id));
        ALTER TABLE c DROP CONSTRAINT k;
        CREATE VIEW v AS SELECT id FROM p;
        CREATE TABLE r (vid int REFERENCES v (id));");
    let tables: Vec<_> = joins(&c, "p", "").into_iter().map(|j| j.0).collect();
    assert_eq!(tables, ["c"]);
    assert!(joins(&c, "v", "").is_empty());
    assert!(joins(&c, "r", "").is_empty());
    // A key to a view's column roots in no table, so comparing it with a key is no mismatch.
    for sql in [
        "SELECT 1 FROM v JOIN c ON v.id = c.pid",
        "SELECT 1 FROM r JOIN c ON r.vid = c.pid",
    ] {
        let (_, diagnostics) = analyze_with_diagnostics(sql, "postgres", Some(&c));
        let mismatches = diagnostics
            .iter()
            .filter(|d| d.rule == Rule::EntityMismatch);
        assert_eq!(mismatches.count(), 0, "{sql}: {diagnostics:?}");
    }
}

#[test]
fn joins_of_views_unknown_tables_and_dangling_keys() {
    let c = pg("CREATE TABLE p (id int PRIMARY KEY);
        CREATE TABLE c (pid int REFERENCES p (id), x int REFERENCES nope (id));
        CREATE VIEW v AS SELECT id FROM p;");
    assert!(joins(&c, "nope", "").is_empty());
    assert!(joins(&c, "v", "").is_empty());
    let tables: Vec<_> = joins(&c, "p", "").into_iter().map(|j| j.0).collect();
    assert_eq!(tables, ["c"]);
}
