//! Completion through `analyzer::complete` and the catalog lookups it reads.

use sqlty_sql::analyzer::{self, ColumnOf, Schema, SchemaData};

fn load(sql: &str, dialect: &str) -> Schema {
    analyzer::load_schema_with_skipped(sql, dialect).0
}

/// `column:relation` for each column whose name starts with `prefix`, at most `limit`.
fn columns(catalog: &SchemaData, prefix: &str, limit: usize) -> Vec<String> {
    catalog
        .columns_with_prefix(prefix, limit)
        .iter()
        .map(|found| {
            let name = match found.column {
                ColumnOf::Table(column) => &column.ident,
                ColumnOf::View(column) => &column.name,
            };
            match (found.qualified, found.relation.schema) {
                (true, Some(schema)) => format!("{name}:{schema}.{}", found.relation.name),
                _ => format!("{name}:{}", found.relation.name),
            }
        })
        .collect()
}

/// `label kind detail` of each candidate at the `|` of `sql`.
fn complete(sql: &str, catalog: &SchemaData, dialect: &str) -> Vec<String> {
    let cursor = sql.find('|').unwrap_or_default();
    let sql = sql.replacen('|', "", 1);
    analyzer::complete(&sql, cursor, dialect, Some(catalog), 50)
        .candidates
        .iter()
        .map(|c| {
            format!(
                "{}:{:?}:{}",
                c.label,
                c.kind,
                c.detail.as_deref().unwrap_or("")
            )
        })
        .collect()
}

#[test]
fn columns_by_prefix_list_the_schema_before_system_catalogs() {
    let catalog = load(
        "CREATE SCHEMA other; CREATE TABLE users (id INT, name TEXT);
        CREATE TABLE other.orders (id INT, nav TEXT); CREATE VIEW names AS SELECT name FROM users;
        CREATE TABLE gone (LIKE missing);",
        "postgres",
    );
    let found = columns(&catalog, "NA", 20);
    assert_eq!(found[..3], ["name:names", "name:users", "nav:other.orders"]);
    assert!(
        found[3..]
            .iter()
            .all(|c| c.contains(":pg_catalog.") || c.contains(":information_schema.")),
        "{found:?}"
    );
    assert!(columns(&catalog, "\"NA", 20).is_empty());
    assert_eq!(columns(&catalog, "na", 2), ["name:names", "name:users"]);
}

#[test]
fn columns_by_prefix_fold_as_the_dialect_does() {
    let catalog = load(r#"CREATE TABLE t ("Kind" TEXT, kin INT);"#, "postgres");
    assert_eq!(columns(&catalog, "\"Ki", 5), [r#""Kind":t"#]);
    for dialect in ["mysql", "sqlite", "duckdb"] {
        let catalog = load(
            "CREATE TABLE Users (Name TEXT); CREATE TABLE b (name_2 INT);
            CREATE VIEW V AS SELECT Name AS FullName FROM Users;",
            dialect,
        );
        assert_eq!(
            columns(&catalog, "NA", 5),
            ["Name:users", "name_2:b"],
            "{dialect}"
        );
        assert_eq!(columns(&catalog, "fu", 5), ["FullName:v"], "{dialect}");
    }
}

#[test]
fn a_column_without_from_completes_from_every_relation() {
    let catalog = load(
        "CREATE SCHEMA other; CREATE TABLE users (id INT, name TEXT NOT NULL);
        CREATE TABLE orders (id INT, user_id INT); CREATE TABLE other.notes (nav INT);
        CREATE VIEW totals AS SELECT user_id AS owner_id, count(*) AS total FROM orders GROUP BY 1;",
        "postgres",
    );
    let pg = |sql: &str| complete(sql, &catalog, "postgres");
    assert_eq!(pg("SELECT owner_|"), ["owner_id:Column:totals"]);
    let found = pg("SELECT na|");
    assert_eq!(found[..2], ["name:Column:users", "nav:Column:other.notes"]);
    assert!(found[2..].iter().all(|c| c.contains(":pg_catalog.")
        || c.contains(":information_schema.")
        || c.contains(":Function:")));
    assert_eq!(pg("SELECT id, user_|")[..1], ["user_id:Column:orders"]);
    assert_eq!(pg("SELECT 1 WHERE user_|")[..1], ["user_id:Column:orders"]);
    assert_eq!(pg("SELECT us| FROM"), pg("SELECT us|"));
    assert_eq!(
        pg("SELECT * FROM users WHERE id IN (SELECT user_|)"),
        Vec::<String>::new()
    );
    assert!(
        pg("SELECT user_| FROM users").is_empty(),
        "a FROM keeps the scope's columns"
    );
    assert!(
        pg("SELECT user_| FROM missing").is_empty(),
        "even an unknown one"
    );
    assert!(
        pg("SELECT u.user_|").is_empty(),
        "a qualifier names a relation"
    );
    assert!(
        !pg("SELECT |").iter().any(|c| c.contains(":Column:")),
        "nothing typed"
    );
}

#[test]
fn from_ranks_the_relations_that_have_the_selected_columns() {
    let catalog = load(
        "CREATE SCHEMA other;
        CREATE TABLE accounts (id INT, email TEXT);
        CREATE TABLE users (id INT, email TEXT, created_at DATE);
        CREATE TABLE events (id INT, created_at DATE);
        CREATE VIEW signups AS SELECT email, created_at FROM users;
        CREATE TABLE other.people (email TEXT, created_at DATE);
        CREATE TABLE zoo (n INT);",
        "postgres",
    );
    let pg = |sql: &str| -> Vec<String> {
        complete(sql, &catalog, "postgres")
            .into_iter()
            .filter(|c| !c.ends_with(":pg_catalog") && !c.contains(":Function:"))
            .collect()
    };
    let ranked = [
        "signups:View:public",
        "users:Table:public",
        "accounts:Table:public",
        "events:Table:public",
        "zoo:Table:public",
    ];
    assert_eq!(pg("SELECT email, created_at FROM |"), ranked);
    assert_eq!(
        pg("SELECT u.email, lower(created_at::text) AS e FROM |"),
        ranked
    );
    assert_eq!(
        pg("SELECT email, created_at, FROM |"),
        ranked,
        "half written"
    );
    assert_eq!(pg("SELECT DISTINCT email e, created_at FROM |"), ranked);
    assert_eq!(
        pg("SELECT email, created_at FROM u|"),
        ["users:Table:public"]
    );
    assert_eq!(
        pg("SELECT n FROM | WHERE"),
        [
            "zoo:Table:public",
            "accounts:Table:public",
            "events:Table:public",
            "signups:View:public",
            "users:Table:public"
        ]
    );
    assert_eq!(
        pg("SELECT nope, 1, * FROM |")[0],
        "accounts:Table:public",
        "no column known: by name"
    );
    assert_eq!(
        pg("SELECT email FROM users JOIN |")[0],
        "accounts:Table:public",
        "JOIN keeps the prefix list"
    );
    let found = complete("SELECT email FROM |", &catalog, "postgres");
    assert_eq!(found[..2], ["accounts:Table:public", "signups:View:public"]);
    let found = pg("WITH recent AS (SELECT 1) SELECT id FROM |");
    assert!(found.contains(&"recent:Cte:".to_string()), "{found:?}");
    assert_eq!(
        pg("SELECT (SELECT max(id) FROM zoo) AS m, n FROM |")[0],
        "zoo:Table:public"
    );
    assert_eq!(
        pg("SELECT EXTRACT(year FROM created_at) FROM |")[0],
        "events:Table:public"
    );
    let functions = complete("SELECT id FROM generate_s|", &catalog, "postgres");
    assert!(
        functions[0].starts_with("generate_series:Function:"),
        "{functions:?}"
    );
}

#[test]
fn columns_and_ctes_match_by_subsequence() {
    let catalog = load(
        "CREATE TABLE orders (id INT, user_id INT, uid_legacy INT, status TEXT, used_at DATE);
        CREATE TABLE users (id INT);",
        "postgres",
    );
    let pg = |sql: &str| -> Vec<String> {
        complete(sql, &catalog, "postgres")
            .into_iter()
            .filter(|c| !c.contains(":Function:"))
            .collect()
    };
    assert_eq!(
        pg("SELECT uid| FROM orders"),
        ["uid_legacy:Column:orders", "user_id:Column:orders"]
    );
    assert_eq!(pg("SELECT u_i| FROM orders"), ["user_id:Column:orders"]);
    assert_eq!(
        pg("SELECT usd| FROM orders"),
        ["used_at:Column:orders", "user_id:Column:orders"],
        "word starts before other subsequences"
    );
    assert_eq!(pg("SELECT UID| FROM orders")[1], "user_id:Column:orders");
    assert_eq!(
        pg("SELECT u_i| FROM orders, (SELECT 1 AS ua_xi) s"),
        ["user_id:Column:orders", "ua_xi:Column:s"]
    );
    assert!(
        pg("SELECT d| FROM orders").is_empty(),
        "the first letter starts a word"
    );
    assert_eq!(
        pg("WITH recent_orders AS (SELECT 1) SELECT * FROM ro|"),
        ["recent_orders:Cte:"]
    );
    assert!(pg("SELECT * FROM ur|").is_empty(), "tables match by prefix");
    let quoted = load(r#"CREATE TABLE t ("Kind" TEXT, kin INT);"#, "postgres");
    let labels: Vec<String> = complete(r#"SELECT "Ki| FROM t"#, &quoted, "postgres");
    assert_eq!(
        labels,
        [r#""Kind":Column:t"#],
        "a quoted word keeps its case"
    );
}

#[test]
fn a_cut_list_is_incomplete() {
    let catalog = load("CREATE TABLE t (b_ax INT, a_xy INT, ax INT);", "postgres");
    let found = analyzer::complete("SELECT ax FROM t", 9, "postgres", Some(&catalog), 2);
    let labels: Vec<_> = found.candidates.iter().map(|c| c.label.as_str()).collect();
    assert_eq!((labels, found.incomplete), (vec!["ax", "a_xy"], true));
}
