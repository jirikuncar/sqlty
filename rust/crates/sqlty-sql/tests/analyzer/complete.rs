use sqlty_sql::analyzer::*;
const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT NOT NULL);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, total NUMERIC);
    CREATE VIEW user_names AS SELECT id, name FROM users;
    CREATE TABLE legacy (LIKE missing);";

/// The labels completing at the `|` of `sql`, as `label:detail`.
fn labels(sql: &str, catalog: &SchemaData, dialect: &str) -> Vec<String> {
    let cursor = sql.find('|').expect("a cursor");
    let sql = sql.replacen('|', "", 1);
    let found = complete(&sql, cursor, dialect, Some(catalog), 50);
    assert!(
        found.candidates.is_empty() || found.replace.end == cursor,
        "{sql}"
    );
    found
        .candidates
        .iter()
        .map(|c| format!("{}:{}", c.label, c.detail.as_deref().unwrap_or("")))
        .collect()
}

fn pg(sql: &str) -> Vec<String> {
    labels(
        sql,
        &load_schema_with_skipped(SCHEMA, "postgres").0,
        "postgres",
    )
}

#[test]
fn relations() {
    assert_eq!(
        pg("SELECT * FROM us|"),
        ["user_names:public", "users:public"]
    );
    assert_eq!(pg("SELECT * FROM users u JOIN o|"), ["orders:public"]);
    assert_eq!(pg("SELECT * FROM users, ord|"), ["orders:public"]);
    assert_eq!(pg("SELECT * FROM public.or|"), ["orders:public"]);
    assert_eq!(pg("UPDATE ord| SET total = 1"), ["orders:public"]);
    assert_eq!(pg("INSERT INTO leg| VALUES (1)"), ["legacy:public"]);
    assert_eq!(
        pg("WITH recent AS (SELECT 1) SELECT * FROM re|"),
        ["recent:"]
    );
}

#[test]
fn joins_on_foreign_keys_come_first() {
    let catalog = load_schema_with_skipped(
        "CREATE TABLE users (id INT PRIMARY KEY, manager_id INT REFERENCES users);
        CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users (id));
        CREATE TABLE items (order_id INT, line INT, PRIMARY KEY (order_id, line),
            FOREIGN KEY (order_id) REFERENCES orders);
        CREATE TABLE notes (order_id INT, line INT,
            FOREIGN KEY (order_id, line) REFERENCES items (order_id, line));
        CREATE SCHEMA billing;
        CREATE TABLE billing.invoices (id INT, order_id INT REFERENCES orders);",
        "postgres",
    )
    .0;
    let pg = |sql: &str| labels(sql, &catalog, "postgres");
    assert_eq!(
        pg("SELECT * FROM orders o JOIN |")[..4],
        [
            "billing.invoices ON billing.invoices.order_id = o.id:billing",
            "items ON items.order_id = o.id:public",
            "users ON users.id = o.user_id:public",
            "items:public",
        ]
    );
    assert_eq!(
        pg("SELECT * FROM orders JOIN us|"),
        ["users ON users.id = orders.user_id:public", "users:public"]
    );
    assert_eq!(
        pg("SELECT * FROM items i JOIN no|"),
        [
            "notes ON notes.order_id = i.order_id AND notes.line = i.line:public",
            "notes:public"
        ]
    );
    assert_eq!(
        pg("SELECT * FROM users JOIN orders o ON o.user_id = users.id JOIN us|"),
        ["users:public"],
        "a name already in scope makes its ON clause ambiguous"
    );
    assert_eq!(
        pg("SELECT * FROM users u JOIN us|"),
        [
            "users ON users.id = u.manager_id:public",
            "users ON users.manager_id = u.id:public",
            "users:public"
        ]
    );
    assert_eq!(pg("SELECT * FROM orders o, us|"), ["users:public"]);
    assert_eq!(
        pg("SELECT * FROM orders o JOIN public.us|"),
        ["users:public"]
    );
}

#[test]
fn schema_relations_come_before_system_catalogs() {
    let found = pg("SELECT * FROM |");
    assert_eq!(
        found[..4],
        [
            "legacy:public",
            "orders:public",
            "user_names:public",
            "users:public"
        ]
    );
    assert!(found[4..]
        .iter()
        .all(|label| label.ends_with(":pg_catalog")));
}

#[test]
fn qualified_columns() {
    assert_eq!(pg("SELECT u.| FROM users u"), ["email:u", "id:u", "name:u"]);
    assert_eq!(pg("SELECT u.na| FROM users u"), ["name:u"]);
    assert_eq!(pg("SELECT users.e| FROM users"), ["email:users"]);
    assert_eq!(
        pg("SELECT o.| FROM users u JOIN orders o ON u.id = o.user_id"),
        ["id:o", "total:o", "user_id:o"]
    );
    assert_eq!(pg("SELECT v.| FROM user_names v"), ["id:v", "name:v"]);
}

#[test]
fn unqualified_columns_in_each_clause() {
    let both = ["o.id:o", "o.total:o", "o.user_id:o", "u.id:u"];
    let from = "FROM users u JOIN orders o ON u.id = o.user_id";
    assert_eq!(pg(&format!("SELECT u.name, | {from}"))[..1], ["o.id:o"]);
    assert_eq!(pg(&format!("SELECT * {from} WHERE i|")), ["id:o", "id:u"]);
    assert_eq!(
        pg(&format!("SELECT * {from} WHERE u.id = 1 AND t|")),
        ["total:o"]
    );
    assert_eq!(pg(&format!("SELECT * {from} ORDER BY |"))[..1], ["o.id:o"]);
    assert_eq!(
        pg(&format!("SELECT count(*) {from} GROUP BY u|")),
        ["user_id:o"]
    );
    assert_eq!(
        pg(&format!("SELECT * {from} WHERE |")),
        [
            "o.id:o",
            "o.total:o",
            "o.user_id:o",
            "u.email:u",
            "u.id:u",
            "u.name:u"
        ]
    );
    assert_eq!(
        pg("SELECT * FROM users WHERE |"),
        ["email:users", "id:users", "name:users"],
        "one relation in scope: its columns need no qualifier"
    );
    assert_eq!(
        pg("SELECT * FROM users u JOIN orders o ON |")
            .into_iter()
            .filter(|l| !l.contains("name") && !l.contains("email"))
            .collect::<Vec<_>>(),
        both
    );
    assert_eq!(pg("UPDATE users SET na| = 'x'"), ["name:users"]);
    assert_eq!(
        pg("INSERT INTO orders (total) VALUES (1) RETURNING us|"),
        ["user_id:orders"]
    );
}

#[test]
fn half_written_queries() {
    assert_eq!(
        pg("SELECT o.| FROM users u JOIN orders o ON u.id ="),
        ["id:o", "total:o", "user_id:o"]
    );
    assert_eq!(pg("SELECT u.na| FROM users u WHERE"), ["name:u"]);
    assert_eq!(pg("SELECT id, | FROM users WHERE id IN (").len(), 3);
    assert!(pg("SELECT |").is_empty(), "no FROM: no columns");
    assert!(pg("SELECT l.| FROM legacy l").is_empty(), "unknown shape");
}

#[test]
fn cte_and_subquery_scope() {
    assert_eq!(
        pg("WITH recent AS (SELECT id AS rid FROM orders) SELECT r.| FROM recent r"),
        ["rid:r"]
    );
    assert_eq!(
        pg("SELECT s.| FROM (SELECT id, total AS amount FROM orders) s"),
        ["amount:s", "id:s"]
    );
    assert_eq!(
        pg("SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.|)"),
        ["email:u", "id:u", "name:u"]
    );
    assert_eq!(
        pg("SELECT * FROM users JOIN orders USING (id) WHERE tot|"),
        ["total:orders"]
    );
}

#[test]
fn nothing_completes() {
    for sql in [
        "SELECT 'us|' FROM users",
        "SELECT 1 -- us|",
        "SELECT * FROM users u|",
        "SELECT * FROM users AS |",
        "SELECT name | FROM users",
        "SELECT count(*) | FROM users",
        "SELECT * FROM (|",
        "INSERT INTO users VALUES (|",
        "us|",
    ] {
        assert!(pg(sql).is_empty(), "{sql}");
    }
}

#[test]
fn candidates_carry_type_and_nullability() {
    let catalog = load_schema_with_skipped(SCHEMA, "postgres").0;
    let found = complete("SELECT u. FROM users u", 9, "postgres", Some(&catalog), 50);
    let email = &found.candidates[0];
    assert_eq!(
        (email.sql_type.as_deref(), email.nullable, email.kind),
        (Some("TEXT"), Some(false), CandidateKind::Column)
    );
    assert_eq!(found.candidates[2].nullable, Some(true));
    let views = complete("SELECT * FROM user_n", 20, "postgres", Some(&catalog), 50);
    assert_eq!(views.candidates[0].kind, CandidateKind::View);
}

#[test]
fn limit_marks_the_list_incomplete() {
    let catalog = load_schema_with_skipped(SCHEMA, "postgres").0;
    let found = complete("SELECT u. FROM users u", 9, "postgres", Some(&catalog), 2);
    assert_eq!((found.candidates.len(), found.incomplete), (2, true));
    let found = complete("SELECT u. FROM users u", 9, "postgres", Some(&catalog), 3);
    assert!(!found.incomplete);
}

#[test]
fn quoted_names() {
    let catalog = load_schema_with_skipped(
        r#"CREATE TABLE "UserLog" ("Kind" TEXT, at INT); CREATE TABLE users (id INT);"#,
        "postgres",
    )
    .0;
    assert_eq!(
        labels(r#"SELECT * FROM "Us|"#, &catalog, "postgres"),
        [r#""UserLog":public"#]
    );
    assert_eq!(
        labels(r#"SELECT l.| FROM "UserLog" l"#, &catalog, "postgres"),
        [r#""Kind":l"#, "at:l"]
    );
    assert_eq!(
        labels(
            r#"SELECT * FROM "UserLog" l JOIN users u ON u.id = l.at WHERE |"#,
            &catalog,
            "postgres"
        ),
        [r#"l."Kind":l"#, "l.at:l", "u.id:u"]
    );
}

#[test]
fn mysql_and_sqlite() {
    for dialect in ["mysql", "sqlite"] {
        let catalog = load_schema_with_skipped(
            "CREATE TABLE Users (id INTEGER PRIMARY KEY, Name TEXT); CREATE TABLE orders (id INTEGER, user_id INTEGER);",
            dialect,
        )
        .0;
        let complete = |sql: &str| labels(sql, &catalog, dialect);
        assert_eq!(complete("SELECT * FROM US|"), ["users:"], "{dialect}");
        assert_eq!(
            complete("SELECT u.| FROM Users u"),
            ["id:u", "Name:u"],
            "{dialect}"
        );
        assert_eq!(
            complete("SELECT * FROM Users u JOIN orders o ON o.user_id = u.|"),
            ["id:u", "Name:u"],
            "{dialect}"
        );
        assert_eq!(
            complete("SELECT n| FROM Users"),
            ["Name:users"],
            "{dialect}"
        );
    }
}

#[test]
fn a_query_that_does_not_parse_completes_from_its_tokens() {
    assert_eq!(
        pg("SELECT | FROM user_names WHERE ("),
        ["id:user_names", "name:user_names"]
    );
    assert_eq!(
        pg("SELECT | FROM users WHERE id IN (SELECT 1 FROM orders) AND ("),
        ["email:users", "id:users", "name:users"]
    );
    assert_eq!(
        pg("SELECT | FROM public.users AS u, orders o WHERE ("),
        [
            "o.id:o",
            "o.total:o",
            "o.user_id:o",
            "u.email:u",
            "u.id:u",
            "u.name:u"
        ]
    );
    assert!(pg("SELECT /* c */ | FROM users WHERE 'x").is_empty());
}
