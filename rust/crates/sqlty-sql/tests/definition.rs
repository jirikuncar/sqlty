//! Go to definition: where the schema or the query defines a name.

use sqlty_sql::analyzer::{definition, load_schema_with_skipped, Definition};

/// What the name at `|` of `sql` is defined as: the query's or the schema's text there.
fn at(schema_sql: &str, dialect: &str, sql: &str) -> Option<String> {
    let cursor = sql.find('|')?;
    let sql = sql.replacen('|', "", 1);
    let (schema, _) = load_schema_with_skipped(schema_sql, dialect);
    Some(
        match definition(&sql, cursor, dialect, Some(&schema), Some(schema_sql))? {
            Definition::Query(range) => format!("query {}", &sql[range]),
            Definition::Schema(range) => {
                let line = schema_sql[..range.start].matches('\n').count() + 1;
                format!("schema {line} {}", &schema_sql[range])
            }
        },
    )
}

const SCHEMA: &str = "CREATE TABLE users (id INT, name TEXT, \"Nick\" TEXT);
CREATE SCHEMA app;
CREATE TABLE app.orders (id INT, user_id INT, total INT);
ALTER TABLE users ADD COLUMN email TEXT;
ALTER TABLE users RENAME COLUMN name TO full_name;
CREATE VIEW user_names AS SELECT id, full_name AS label FROM users;
CREATE TABLE old (id INT);
ALTER TABLE old RENAME TO archived;";

fn pg(sql: &str) -> Option<String> {
    at(SCHEMA, "postgres", sql)
}

#[test]
fn schema_names() {
    let found = [
        pg("SELECT 1 FROM us|ers"),
        pg("SELECT 1 FROM users WHERE i|d = 1"),
        pg("SELECT u.ema|il FROM users u"),
        pg("SELECT full_n|ame FROM users"),
        pg("SELECT \"Ni|ck\" FROM users"),
        pg("SELECT 1 FROM app.ord|ers"),
        pg("SELECT o.tot|al FROM app.orders o"),
        pg("SELECT 1 FROM user_na|mes"),
        pg("SELECT lab|el FROM user_names"),
        pg("SELECT 1 FROM archi|ved"),
        pg("SELECT users.i|d FROM users"),
        pg("UPDATE use|rs SET email = ''"),
        pg("INSERT INTO users (id, ema|il) VALUES (1, '')"),
        pg("SELECT 1 FROM app.orders o JOIN users u USING (i|d)"),
    ];
    assert_eq!(
        found.map(Option::unwrap_or_default),
        [
            "schema 1 users",
            "schema 1 id",
            "schema 4 email",
            "schema 5 full_name",
            "schema 1 \"Nick\"",
            "schema 3 orders",
            "schema 3 total",
            "schema 6 user_names",
            "schema 6 label",
            "schema 8 archived",
            "schema 1 id",
            "schema 1 users",
            "schema 4 email",
            "schema 1 id",
        ]
        .map(str::to_string)
    );
}

#[test]
fn query_names() {
    let found = [
        pg("SELECT u|.id FROM users u"),
        pg("SELECT 1 FROM users |u"),
        pg("WITH big AS (SELECT 1 AS n) SELECT n FROM b|ig"),
        pg("WITH big AS (SELECT 1 AS n) SELECT b|ig.n FROM big"),
        pg("SELECT s|.total FROM (SELECT 1 AS total) s"),
        pg("WITH big (|m) AS (SELECT 1) SELECT 1 FROM big"),
        pg("WITH b|ig AS (SELECT 1) SELECT 1 FROM big"),
        pg("WITH big (m, n) AS MATERIALIZED (SELECT 1, 2) SELECT big|.n FROM big"),
        pg("SELECT u|.id FROM users AS u, LATERAL (SELECT 1) l"),
        pg("SELECT u|.id FROM (SELECT 1 AS id), users u"),
        pg("SELECT u|.id FROM users u WHERE EXISTS (SELECT 1 FROM users u)"),
        pg("SELECT users|.id FROM users u"),
        pg("WITH c AS (SELECT 1 AS x) SELECT c|.x FROM c AS d"),
        pg("SELECT o|.id FROM (users JOIN app.orders o ON true)"),
        pg("SELECT 1 FROM (SELECT u|.id FROM users u) s"),
        pg("SELECT s|.id FROM (SELECT u.id FROM users u) s, users"),
    ];
    assert_eq!(
        found.map(Option::unwrap_or_default),
        [
            "query u",
            "query u",
            "query big",
            "query big",
            "query s",
            "query m",
            "query big",
            "query big",
            "query u",
            "query u",
            "query u",
            "schema 1 users",
            "query c",
            "query o",
            "query u",
            "query s",
        ]
        .map(str::to_string)
    );
}

#[test]
fn output_columns() {
    let found = [
        pg("WITH big AS (SELECT 1 AS n) SELECT big.|n FROM big"),
        pg("WITH big AS (SELECT 1 AS n) SELECT |n FROM big"),
        pg("WITH big (m) AS (SELECT 1) SELECT |m FROM big"),
        pg("SELECT s.tot|al FROM (SELECT 1 AS total) s"),
        pg("SELECT s.|total FROM (SELECT SUM(total) AS total FROM app.orders) s"),
        pg("SELECT t.|n FROM (SELECT n FROM (SELECT 2 AS n) i) t"),
        pg("SELECT t.n FROM (SELECT i.|n FROM (SELECT 2 AS n) i) t"),
        pg("SELECT t.n FROM (SELECT i.|n FROM (SELECT total AS n FROM app.orders) i) t"),
        pg("SELECT id AS spent FROM users ORDER BY spe|nt"),
        pg("SELECT 1 AS id FROM users ORDER BY |id"),
        pg("SELECT id, full_name FROM users ORDER BY |2"),
        pg("SELECT id FROM users UNION SELECT id FROM app.orders ORDER BY |1"),
        pg("SELECT COUNT(*) AS n FROM users GROUP BY |n"),
        pg("SELECT DISTINCT ON (id) id AS k FROM users ORDER BY |k"),
        pg("SELECT DISTINCT ON (lower(full_name)) id AS k FROM users ORDER BY |k"),
        pg("SELECT id FROM users ORDER BY full_n|ame"),
        pg("SELECT * FROM (SELECT id AS k FROM users ORDER BY |k) s"),
        pg("SELECT id k FROM users ORDER BY |k"),
        pg("SELECT (id)k FROM users ORDER BY |k"),
        pg("SELECT ROW_NUMBER() OVER (PARTITION BY |id) FROM users"),
        pg("SELECT id FROM users GROUP BY id HAVING true, |id"),
        pg("SELECT DISTINCT id AS k FROM users ORDER BY |k"),
        pg("SELECT 'a', id AS k FROM users ORDER BY |2"),
        pg("SELECT /* x */ 'a' AS k FROM users ORDER BY |k"),
        pg("SELECT 'x', id FROM users ORDER BY |1"),
        pg("(SELECT id AS k FROM users) UNION SELECT 1 ORDER BY |1"),
        pg("SELECT 1; SELECT id AS k FROM users ORDER BY |k"),
        pg("WITH c (x) AS (SELECT 1 AS y, 2 AS z) SELECT c.|z FROM c"),
        pg("SELECT s.|x FROM (SELECT 1 AS y) AS s (x)"),
        pg("SELECT u.|k FROM users u(k)"),
        pg("SELECT u.|full_name FROM users u(k)"),
    ];
    assert_eq!(
        found.map(Option::unwrap_or_default),
        [
            "query 1 AS n",
            "query 1 AS n",
            "query m",
            "query 1 AS total",
            "query SUM(total) AS total",
            "query n",
            "query 2 AS n",
            "query total AS n",
            "query id AS spent",
            "query 1 AS id",
            "schema 5 full_name",
            "schema 1 id",
            "query COUNT(*) AS n",
            "query id AS k",
            "query id AS k",
            "schema 5 full_name",
            "query id AS k",
            "query id k",
            "query (id)k",
            "schema 1 id",
            "schema 1 id",
            "query id AS k",
            "query id AS k",
            "query 'a' AS k",
            "query 'x'",
            "query id AS k",
            "query id AS k",
            "query 2 AS z",
            "query x",
            "query k",
            "schema 5 full_name",
        ]
        .map(str::to_string)
    );
}

#[test]
fn the_example_of_the_output_columns_issue() {
    let schema = "CREATE TABLE orders (user_id INT, total INT);";
    let sql = "WITH big AS (SELECT user_id, SUM(total) AS spent FROM orders GROUP BY user_id)
SELECT user_id, spent FROM big ORDER BY 2";
    let found = [
        at(
            schema,
            "postgres",
            &sql.replacen("spent FROM big", "spe|nt FROM big", 1),
        ),
        at(schema, "postgres", &sql.replacen("BY 2", "BY |2", 1)),
        at(
            schema,
            "postgres",
            &sql.replacen("SELECT user_id, spent", "SELECT user_|id, spent", 1),
        ),
    ];
    assert_eq!(
        found.map(Option::unwrap_or_default),
        [
            "query SUM(total) AS spent",
            "query SUM(total) AS spent",
            "query user_id",
        ]
        .map(str::to_string)
    );
}

#[test]
fn nothing_for_what_neither_defines() {
    assert_eq!(pg("SELECT 1 FROM users ORDER BY |9"), None);
    assert_eq!(pg("SELECT 1 FROM users ORDER BY |0"), None);
    assert_eq!(pg("SELECT id, FROM users ORDER BY |2"), None);
    assert_eq!(pg("SELECT s.|a FROM (VALUES (1)) s"), None);
    assert_eq!(pg("WITH c (x) AS (SELECT 1 AS y) SELECT c.|y FROM c"), None);
    assert_eq!(pg("SELECT s.|y FROM (SELECT 1 AS y) s(x)"), None);
    assert_eq!(pg("SELECT u.|id FROM users u(k)"), None);
    assert_eq!(pg("SELECT *, id FROM users ORDER BY |2"), None);
    assert_eq!(pg("SELECT u.* FROM users u ORDER BY |1"), None);
    assert_eq!(pg("SELECT id > 1 OR k FROM users ORDER BY |k"), None);
    assert_eq!(
        pg("SELECT CASE WHEN true THEN 1 END FROM users ORDER BY |end"),
        None
    );
    assert_eq!(pg("SEL|ECT 1"), None);
    assert_eq!(pg("SELECT 1 FROM nowh|ere"), None);
    assert_eq!(pg("SELECT 1 FROM pg_cl|ass"), None);
    assert_eq!(pg("SELECT 1 FROM a|pp.orders"), None);
    assert_eq!(pg("SELECT nope| FROM users"), None);
    assert_eq!(pg("SELECT 1 FROM generate_ser|ies(1, 2) g"), None);
    assert_eq!(
        pg("SELECT id, ema|il AS e FROM (SELECT * FROM users) s"),
        None
    );
    assert_eq!(pg("SELECT 1 FROM users TABLESAM|PLE bernoulli (1)"), None);
    assert_eq!(pg("WITH |a (x) SELECT 1"), None);
    assert_eq!(pg("WITH |a AS SELECT 1"), None);
    assert_eq!(
        pg("(SELECT |id FROM users)"),
        Some("schema 1 id".to_string())
    );
    assert_eq!(
        pg("WITH a AS (SELECT 1), b (|x) AS (SELECT 2) SELECT 1"),
        Some("query x".to_string())
    );
    assert_eq!(
        pg("WITH a AS NOT MATERIALIZED (SELECT 1) SELECT 1 FROM |a"),
        Some("query a".to_string())
    );
    assert_eq!(
        pg("SELECT u|.id FROM users u, (SELECT 1"),
        Some("query u".to_string())
    );
    assert_eq!(
        pg("SELECT u|.id FROM users u, 'x'"),
        Some("query u".to_string())
    );
    let unknown = "CREATE TABLE u (LIKE missing);";
    assert_eq!(
        at(unknown, "postgres", "SELECT 1 FROM |u"),
        Some("schema 1 u".to_string())
    );
    assert_eq!(at(unknown, "postgres", "SELECT u.|a FROM u"), None);
    let (schema, _) = load_schema_with_skipped(SCHEMA, "postgres");
    assert_eq!(
        definition("SELECT 1 FROM users", 15, "postgres", Some(&schema), None),
        Some(Definition::Schema(0..0))
    );
    assert_eq!(
        definition("SELECT 1 FROM users", 15, "postgres", None, None),
        None
    );
}

#[test]
fn each_dialect() {
    let schema =
        "CREATE TABLE Users (Id INT, `Name` TEXT);\nALTER TABLE Users ADD COLUMN email TEXT;";
    assert_eq!(
        at(schema, "mysql", "SELECT na|me FROM users"),
        Some("schema 1 `Name`".to_string())
    );
    assert_eq!(
        at(schema, "mysql", "SELECT ema|il FROM USERS"),
        Some("schema 2 email".to_string())
    );
    let schema = "CREATE TABLE t (a INT);\nALTER TABLE t RENAME COLUMN a TO b;";
    assert_eq!(
        at(schema, "sqlite", "SELECT |b FROM t"),
        Some("schema 2 b".to_string())
    );
    assert_eq!(
        at(schema, "duckdb", "SELECT |a FROM t"),
        Some("schema 1 a".to_string())
    );
}

#[test]
fn the_statement_that_last_defined_the_name() {
    let schema = "CREATE TABLE t (a INT);
DROP TABLE t;
CREATE TABLE t (b INT, a INT);
CREATE TABLE copy (LIKE t, c INT);
CREATE TABLE summary AS SELECT a FROM t;
CREATE TABLE IF NOT EXISTS t (z INT);
CREATE TABLE child (d INT) INHERITS (t);
CREATE SCHEMA gone;
CREATE TABLE gone.x (id INT);
DROP SCHEMA gone CASCADE;
CREATE TABLE late (id INT REFERENCES later (id));
CREATE TABLE later (id INT PRIMARY KEY);";
    let found = [
        at(schema, "postgres", "SELECT |a FROM t"),
        at(schema, "postgres", "SELECT |a FROM copy"),
        at(schema, "postgres", "SELECT |c FROM copy"),
        at(schema, "postgres", "SELECT 1 FROM summ|ary"),
        at(schema, "postgres", "SELECT |a FROM summary"),
        at(schema, "postgres", "SELECT |a FROM child"),
        at(schema, "postgres", "SELECT |d FROM child"),
        at(schema, "postgres", "SELECT 1 FROM gone.|x"),
        at(schema, "postgres", "SELECT |id FROM late"),
    ];
    assert_eq!(
        found.map(Option::unwrap_or_default),
        [
            "schema 3 a",
            "schema 4 ",
            "schema 4 c",
            "schema 5 summary",
            "schema 5 a",
            "schema 3 a",
            "schema 7 d",
            "",
            "schema 11 id",
        ]
        .map(str::to_string)
    );
}
