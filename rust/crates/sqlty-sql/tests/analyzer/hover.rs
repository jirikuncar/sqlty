use sqlty_sql::analyzer::*;
const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, \"Nick\" TEXT);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total INTEGER);
    CREATE VIEW user_names AS SELECT id, name FROM users;";

/// The hover at the `|` of `sql`, with the word it covers.
fn at(sql: &str) -> Option<(String, String)> {
    let cursor = sql.find('|').expect("a cursor");
    let sql = sql.replacen('|', "", 1);
    let schema = load_schema_with_skipped(SCHEMA, "postgres").0;
    let found = hover(&sql, cursor, "postgres", Some(&schema))?;
    Some((sql[found.range].to_string(), found.contents))
}

fn contents(sql: &str) -> String {
    at(sql).map(|(_, contents)| contents).unwrap_or_default()
}

#[test]
fn a_column_shows_its_type_and_source() {
    assert_eq!(
        at("SELECT 1 FROM users u WHERE u.na|me = ''"),
        Some((
            "name".to_string(),
            "Column `name` of `u`: `TEXT NULL`".to_string()
        ))
    );
    assert_eq!(
        contents("SELECT 1 FROM users WHERE |id = 1"),
        "Column `id` of `users`: `INT NOT NULL`"
    );
    assert_eq!(
        contents("SELECT 1 FROM users WHERE \"Nick|\" = ''"),
        "Column `\"Nick\"` of `users`: `TEXT NULL`"
    );
}

#[test]
fn a_table_lists_its_columns_in_order() {
    assert_eq!(
        contents("SELECT 1 FROM ord|ers"),
        "Table `public.orders`\n- `id` `INT NOT NULL`\n- `user_id` `INT NOT NULL`\n- `total` `INT NULL`"
    );
    assert_eq!(
        contents("SELECT 1 FROM user_names|"),
        "View `public.user_names`\n- `id` `INT NOT NULL`\n- `name` `TEXT NULL`"
    );
    assert_eq!(
        contents("WITH recent AS (SELECT 1) SELECT * FROM rec|ent"),
        "CTE `recent` of the query"
    );
}

#[test]
fn a_qualifier_lists_the_columns_of_its_relation() {
    assert_eq!(
        contents("SELECT 1 FROM orders o WHERE |o.total > 1"),
        "Relation `o`\n- `id` `INT NOT NULL`\n- `total` `INT NULL`\n- `user_id` `INT NOT NULL`"
    );
}

#[test]
fn a_select_item_shows_its_result_column_and_rules() {
    let text = contents("SELECT SUM(total) AS tot|al FROM orders");
    assert!(
        text.starts_with(
            "Result column `total`: `int | None` (`BIGINT`)\n\nTyped by `postgres.aggregate.sum-integers`: "
        ),
        "{text}"
    );
    assert_eq!(
        contents("SELECT id, na|me FROM users"),
        "Result column `name`: `str | None` (`TEXT`)"
    );
    let text = contents("WITH x AS (SELECT 1 AS a) SELECT a, a + 1 AS |b FROM x");
    assert!(
        text.starts_with("Result column `b`: `int` (`INT`)\n\nTyped by `common.arithmetic."),
        "{text}"
    );
}

#[test]
fn a_name_inside_a_select_item_is_its_column() {
    assert_eq!(
        contents("SELECT SUM(tot|al) AS n FROM orders"),
        "Column `total` of `orders`: `INT NULL`"
    );
}

#[test]
fn a_star_before_the_item_hides_its_result_column() {
    assert_eq!(
        contents("SELECT *, na|me FROM users"),
        "Column `name` of `users`: `TEXT NULL`"
    );
    assert_eq!(
        contents("SELECT na|me, * FROM users"),
        "Result column `name`: `str | None` (`TEXT`)"
    );
    assert_eq!(
        contents("SELECT DISTINCT ON (id) na|me FROM users"),
        "Result column `name`: `str | None` (`TEXT`)"
    );
}

#[test]
fn nothing_for_keywords_unknown_names_or_dml() {
    assert_eq!(at("SEL|ECT 1"), None);
    assert_eq!(at("SELECT 1 FROM nowh|ere"), None);
    assert_eq!(at("SELECT 1 FROM users u WHERE u.nope| = 1"), None);
    assert_eq!(at("SELECT 1 FROM users WHERE id = 'te|xt'"), None);
    assert_eq!(
        contents("INSERT INTO orders (user_id) SELECT id FROM users RETURNING to|tal"),
        "Column `total` of `orders`: `INT NULL`"
    );
}

fn contents_in(schema: &str, dialect: &str, sql: &str) -> String {
    let cursor = sql.find('|').expect("a cursor");
    let sql = sql.replacen('|', "", 1);
    let schema = load_schema_with_skipped(schema, dialect).0;
    hover(&sql, cursor, dialect, Some(&schema)).map_or_else(String::new, |found| found.contents)
}

#[test]
fn rarer_shapes() {
    let wide = format!(
        "CREATE TABLE wide ({});",
        (0..51)
            .map(|n| format!("c{n} INT"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let views = "CREATE TABLE t (a INT, b TEXT);
        CREATE VIEW v AS SELECT NULL AS x, a FROM t;";
    let found = [
        contents_in(&wide, "postgres", "SELECT 1 FROM wi|de")
            .lines()
            .last()
            .unwrap()
            .to_string(),
        contents_in(&wide, "postgres", "SELECT 1 FROM wide w WHERE |w.c0 = 1")
            .lines()
            .last()
            .unwrap()
            .to_string(),
        contents_in(views, "sqlite", "SELECT 1 FROM |t"),
        contents_in(views, "sqlite", "SELECT 1 FROM |v"),
        contents_in(views, "sqlite", "SELECT 1 FROM v WHERE |x IS NULL"),
        contents_in(views, "sqlite", "SELECT (zz|z) AS q FROM t"),
        contents_in(views, "sqlite", "SELECT NULL AS |q FROM t"),
        contents_in(
            views,
            "sqlite",
            "SELECT u.a AS |q FROM t LEFT JOIN t u ON true",
        )
        .lines()
        .take(3)
        .collect::<Vec<_>>()
        .join("|"),
        contents_in(views, "sqlite", "UPDATE t SET |a = 1"),
        contents_in(views, "sqlite", "SELECT DISTINCT ON ((a), b) |b FROM t"),
        contents_in(
            "CREATE SCHEMA s; CREATE TABLE s.t (a INT);",
            "postgres",
            "SELECT 1 FROM |s.t",
        ),
        contents_in(views, "postgres", "SELECT a::geometry AS |q FROM t"),
        contents_in(views, "sqlite", "SELECT json_extract(b, '$') AS |q FROM t")
            .lines()
            .next()
            .unwrap()
            .to_string(),
        contents_in(
            "CREATE VIEW p AS SELECT $1::int AS p;",
            "postgres",
            "SELECT 1 FROM |p",
        ),
        contents_in(views, "postgres", "SELECT DISTINCT |a FROM t"),
    ];
    assert_eq!(
        found,
        [
            "- ...",
            "- ...",
            "Table `t`\n- `a` `INT NULL`\n- `b` `TEXT NULL`",
            "View `v`\n- `x`\n- `a` `INT NULL`",
            "Column `x` of `v`",
            "Result column `q`",
            "Result column `q`",
            "Result column `q`: `int | None` (`INT`)",
            "Column `a` of `t`: `INT NULL`",
            "Result column `b`: `str | None` (`TEXT`)",
            "",
            "Result column `q`: `GEOMETRY`",
            "Result column `q`: `typing.Any`",
            "View `public.p`\n- `p` `INT`",
            "Result column `a`: `int | None` (`INT`)",
        ]
    );
}
