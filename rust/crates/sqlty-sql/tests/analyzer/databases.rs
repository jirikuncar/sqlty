use crate::support::*;
use sqlty_sql::analyzer::*;
use std::sync::Arc;
fn project() -> Schema {
    let analytics = load_schema("CREATE TABLE events (n INTEGER NOT NULL)", "sqlite");
    SchemaData::with_databases(
        load_schema("CREATE TABLE users (id TEXT NOT NULL)", "postgres"),
        Databases {
            name: DEFAULT.to_string(),
            absent: false,
            others: vec![
                ("analytics".to_string(), "sqlite", Some(analytics)),
                ("cache".to_string(), "mysql", None),
            ],
        },
    )
    .unwrap()
}

#[test]
fn the_first_line_names_the_database() {
    assert_eq!(
        database_name("--sql sqlty:db=analytics\nSELECT 1"),
        Some("analytics")
    );
    assert_eq!(
        database_name("--sql sqlty:db=a-b_1\\nSELECT 1"),
        Some("a-b_1")
    );
    assert_eq!(database_name("  -- sql sqlty:db=x\nSELECT 1"), Some("x"));
    assert_eq!(
        database_name("-- sqlty: db=x\n-- name: q\nSELECT 1"),
        Some("x")
    );
    assert_eq!(database_name("--sql\nSELECT 'sqlty:db=x'"), None);
    assert_eq!(database_name("--sql\\nSELECT 1 -- sqlty:db=x"), None);
    assert_eq!(database_name("SELECT 1 -- sqlty:db=x"), None);
}

#[test]
fn a_query_is_read_against_the_database_it_names() {
    let schema = project();
    let infer = |sql| infer_type(sql, "postgres", Some(&*schema));
    assert_eq!(
        infer("--sql\nSELECT id FROM users").as_deref(),
        Some("tuple[str]")
    );
    assert_eq!(
        infer("--sql sqlty:db=analytics\nSELECT n FROM events").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("--sql sqlty:db=default\nSELECT id FROM users").as_deref(),
        Some("tuple[str]")
    );
    let (dialect, routed) = route("--sql sqlty:db=cache\nSELECT 1", "postgres", Some(&*schema));
    assert_eq!((dialect, routed.is_none()), ("mysql", true));
}

#[test]
fn an_unknown_name_is_read_against_the_schema_itself() {
    let schema = project();
    let sql = "--sql sqlty:db=analytic\nSELECT id FROM users";
    assert_eq!(route(sql, "postgres", Some(&*schema)).0, "postgres");
    assert_eq!(
        unknown_database(sql, Some(&*schema)).as_deref(),
        Some("database `analytic` is not configured; did you mean `analytics`?")
    );
    assert_eq!(
        unknown_database("--sql sqlty:db=cache\nSELECT 1", Some(&*schema)),
        None
    );
    assert_eq!(
        unknown_database("--sql sqlty:db=default\nSELECT 1", None),
        None
    );
    assert!(unknown_database("--sql sqlty:db=x\nSELECT 1", None).is_some());
}

#[test]
fn a_shared_schema_takes_no_databases() {
    let schema = project();
    let _shared = Arc::clone(&schema);
    assert!(SchemaData::with_databases(schema, Databases::default()).is_none());
}
