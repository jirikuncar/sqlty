//! Which tables and views complete after `FROM`: prefix, search path, schema and dialect.

use sqlty_sql::analyzer::{complete, load_schema_with_skipped, CandidateKind, Schema};

fn load(sql: &str, dialect: &str) -> Schema {
    load_schema_with_skipped(sql, dialect).0
}

/// `schema.name:kind` of what completes at the end of `query`, in the order offered.
fn offered(schema: &Schema, dialect: &str, query: &str, limit: usize) -> Vec<String> {
    complete(query, query.len(), dialect, Some(schema), limit)
        .candidates
        .iter()
        .map(|c| {
            let kind = match c.kind {
                CandidateKind::Table => "Table",
                CandidateKind::View => "View",
                _ => "Other",
            };
            match &c.detail {
                Some(schema) => format!("{schema}.{}:{kind}", c.label),
                None => format!("{}:{kind}", c.label),
            }
        })
        .collect()
}

fn pg(sql: &str) -> Schema {
    load(sql, "postgres")
}

#[test]
fn prefix_folds_like_the_catalog() {
    let c = pg(
        r#"CREATE TABLE Users (id int); CREATE TABLE "UserLog" (id int);
        CREATE TABLE usage (id int);"#,
    );
    let found = |prefix: &str| offered(&c, "postgres", &format!("SELECT * FROM {prefix}"), 10);
    assert_eq!(found("US"), ["public.usage:Table", "public.users:Table"]);
    assert_eq!(found("\"User"), ["public.\"UserLog\":Table"]);
    assert!(found("\"userl").is_empty());
}

#[test]
fn a_bare_prefix_follows_the_search_path() {
    let c = pg("CREATE SCHEMA app; SET search_path TO app, public;
        CREATE TABLE users (id int); CREATE TABLE public.users (id text);
        CREATE TABLE public.user_roles (id int); CREATE TABLE other.user_secrets (id int);");
    let found = |prefix: &str| offered(&c, "postgres", &format!("SELECT * FROM {prefix}"), 10);
    assert_eq!(
        found("user"),
        ["public.user_roles:Table", "app.users:Table"]
    );
    assert_eq!(found("pg_clas"), ["pg_catalog.pg_class:Table"]);
}

#[test]
fn visible_relations_list_the_path_first() {
    let c = pg("CREATE SCHEMA app; SET search_path TO app, public;
        CREATE TABLE users (id int); CREATE TABLE public.users (id text);
        CREATE TABLE public.pg_users (id int); CREATE TABLE public.user_roles (id int);
        CREATE TABLE public.pg_class (id int); CREATE TABLE other.user_secrets (id int);");
    let found = offered(&c, "postgres", "SELECT * FROM ", 4);
    // The path's schemas fill the limit before pg_catalog; completion sorts them by name.
    assert_eq!(
        found,
        [
            "public.pg_users:Table",
            "public.user_roles:Table",
            "app.users:Table",
            "pg_catalog.pg_attrdef:Table"
        ]
    );
    assert_eq!(
        offered(&c, "postgres", "SELECT * FROM pg_cla", 10),
        ["pg_catalog.pg_class:Table"]
    );
    assert_eq!(
        offered(&c, "postgres", "SELECT * FROM USER", 10),
        ["public.user_roles:Table", "app.users:Table"]
    );
}

#[test]
fn a_schema_narrows_to_one_schema() {
    let c = pg(
        "CREATE TABLE users (id int); CREATE TABLE other.users (id int);
        CREATE TABLE other.user_secrets (id int); CREATE TABLE \"Other\".users (id int);",
    );
    let found = |from: &str| offered(&c, "postgres", &format!("SELECT * FROM {from}"), 10);
    assert_eq!(
        found("other.us"),
        ["other.user_secrets:Table", "other.users:Table"]
    );
    assert_eq!(found("OTHER.").len(), 2);
    assert_eq!(
        found("information_schema.col"),
        ["information_schema.columns:Table"]
    );
    assert!(found("col").is_empty());
}

#[test]
fn views_are_offered_beside_tables() {
    let c = pg(
        "CREATE TABLE ta (id int, name text); CREATE VIEW tv AS SELECT id FROM ta;
        CREATE TABLE tu (LIKE missing);",
    );
    assert_eq!(
        offered(&c, "postgres", "SELECT * FROM t", 10),
        ["public.ta:Table", "public.tu:Table", "public.tv:View"]
    );
}

#[test]
fn the_limit_caps_the_result() {
    let c = pg("CREATE TABLE a1 (id int); CREATE TABLE a2 (id int); CREATE TABLE a3 (id int);");
    let found = |limit| complete("SELECT * FROM a", 15, "postgres", Some(&c), limit);
    assert_eq!(found(0).candidates.len(), 0);
    assert!(found(0).incomplete);
    assert_eq!(found(2).candidates.len(), 2);
    assert!(found(2).incomplete);
    assert_eq!(found(3).candidates.len(), 3);
    assert!(!found(3).incomplete);
}

#[test]
fn other_dialects_have_no_schemas() {
    for dialect in ["mysql", "sqlite"] {
        let c = load(
            "CREATE TABLE Users (id INT); CREATE TABLE db.usage (id INT);",
            dialect,
        );
        let found = offered(&c, dialect, "SELECT * FROM db.`US", 10);
        assert_eq!(found, ["usage:Table", "users:Table"], "{dialect}");
    }
}
