use crate::support::*;

const DDL: &str = "CREATE TABLE a (id INT NOT NULL, x TEXT NOT NULL, shared TEXT); \
                   CREATE TABLE b (id INT NOT NULL, y TEXT, shared TEXT NOT NULL); \
                   CREATE TABLE c (id INT, z TEXT NOT NULL); \
                   CREATE VIEW v AS SELECT id, x FROM a";

#[test]
fn star_expansion_over_joins() {
    let queries = [
        "SELECT * FROM a",
        "SELECT * FROM a JOIN b ON a.id = b.id",
        "SELECT * FROM a JOIN b USING (id)",
        "SELECT * FROM a NATURAL JOIN b",
        "SELECT a.*, b.y FROM a, b",
        "SELECT b.* FROM a JOIN b ON true",
        "SELECT * FROM a, b, c",
        "SELECT * FROM (a JOIN b USING (id)) JOIN c USING (id)",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn nullability_follows_the_join_kind() {
    let queries = [
        "SELECT a.id, b.id AS bid FROM a LEFT JOIN b ON a.id = b.id",
        "SELECT a.id, b.id AS bid FROM a RIGHT JOIN b ON a.id = b.id",
        "SELECT a.id, b.id AS bid FROM a FULL JOIN b ON a.id = b.id",
        "SELECT a.id, b.id AS bid FROM a INNER JOIN b ON a.id = b.id",
        "SELECT id FROM a LEFT JOIN b USING (id)",
        "SELECT id FROM a RIGHT JOIN b USING (id)",
        "SELECT b.* FROM a LEFT JOIN b ON true",
        "SELECT c.z FROM a LEFT JOIN b ON true LEFT JOIN c ON true",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn derived_tables_ctes_and_views() {
    let queries = [
        "SELECT * FROM (SELECT id, x FROM a) AS d",
        "SELECT d.n FROM (SELECT id AS n FROM a) AS d",
        "SELECT * FROM (SELECT id, x FROM a) AS d(k, v)",
        "WITH w AS (SELECT id FROM a) SELECT * FROM w",
        "WITH w(k) AS (SELECT id FROM a) SELECT k FROM w",
        "WITH a AS (SELECT 1 AS only_here) SELECT * FROM a",
        "SELECT * FROM v",
        "WITH v AS (SELECT 'shadow'::TEXT AS s) SELECT * FROM v",
        "SELECT * FROM (SELECT 1)",
        "SELECT s.* FROM (SELECT 1 AS one) AS s",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn opaque_sources_explain_themselves() {
    let queries = [
        "SELECT * FROM generate_series(1, 3) AS g",
        "SELECT g FROM generate_series(1, 3) AS g",
        "SELECT * FROM unnest(ARRAY[1, 2]) AS u(n)",
        "SELECT * FROM pg_catalog.pg_tables",
        "SELECT tablename FROM pg_tables",
        "SELECT a.id, g FROM a, generate_series(1, 3) AS g",
        "SELECT * FROM ST_Dump(NULL) AS d",
        "SELECT d.path FROM ST_Dump(NULL) AS d",
        "SELECT * FROM a, LATERAL (SELECT a.id + 1 AS next) AS l",
        "SELECT * FROM missing",
        "SELECT * FROM missing AS m JOIN a ON true",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn correlated_subqueries_see_the_outer_scope() {
    let queries = [
        "SELECT (SELECT max(b.id) FROM b WHERE b.id = a.id) AS m FROM a",
        "SELECT (SELECT x) AS m FROM a",
        "SELECT EXISTS (SELECT 1 FROM b WHERE b.id = a.id) AS e FROM a",
        "SELECT * FROM a WHERE id IN (SELECT id FROM b WHERE b.shared = a.shared)",
        "SELECT (SELECT y FROM b WHERE b.id = a.nope) AS m FROM a",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn table_names_fold_by_dialect() {
    let ddl_postgres = "CREATE TABLE \"Mixed\" (id INT NOT NULL); CREATE TABLE lower (id INT)";
    let queries = [
        "SELECT id FROM \"Mixed\"",
        "SELECT id FROM Mixed",
        "SELECT id FROM LOWER",
        "SELECT id FROM public.lower",
        "SELECT id FROM other.lower",
    ];
    let postgres = reports(&queries, "postgres", Some(ddl_postgres));
    let ddl_mysql = "CREATE TABLE Mixed (id INT NOT NULL)";
    let mysql = reports(&queries[..2], "mysql", Some(ddl_mysql));
    insta::assert_snapshot!(format!("postgres:\n{postgres}\nmysql:\n{mysql}"));
}

const SOURCES: &str = "CREATE TABLE users (id INT NOT NULL, name TEXT); \
                       CREATE TABLE t (id INT NOT NULL); \
                       CREATE TABLE a (id INT NOT NULL, kind TEXT, x INT); \
                       CREATE TABLE b (id INT NOT NULL, kind TEXT, x INT); \
                       CREATE TABLE c (id INT NOT NULL, y INT)";

#[test]
fn named_tables_are_read_by_alias_or_name() {
    let queries = [
        "SELECT 1 FROM Users",
        "SELECT U.id FROM users AS U",
        "SELECT users.id FROM public.users",
        "SELECT users.id FROM users AS u",
        "SELECT x.id FROM Public.T AS X",
        "SELECT other.id FROM users",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(SOURCES)));
}

#[test]
fn sources_that_cannot_be_read_are_opaque() {
    let queries = [
        "SELECT g.x FROM generate_series(1, 3) AS g",
        "SELECT * FROM ST_Dump(NULL) AS g",
        "SELECT * FROM (SELECT 1)",
        "SELECT * FROM unnest(ARRAY[1]) AS u",
        "SELECT j.a FROM JSON_TABLE('[]', '$' COLUMNS (a int PATH '$')) AS J",
        "SELECT * FROM users, unnest(ARRAY[1])",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(SOURCES)));
}

#[test]
fn joins_list_every_relation_and_merge_using_columns() {
    let queries = [
        "SELECT a.id, b.id FROM a, b",
        "SELECT a.id, b.id, c.id FROM a JOIN b ON true LEFT JOIN c ON true",
        "SELECT a.id, b.id, c.id FROM (a JOIN b ON true) JOIN c ON true",
        "SELECT s.id, a.id FROM (SELECT 1 AS id) AS s, a",
        "SELECT * FROM a JOIN b USING (Id, kind)",
        "SELECT * FROM a NATURAL JOIN b",
        "SELECT * FROM (a JOIN b USING (x)) JOIN c USING (y)",
        "SELECT * FROM a JOIN b ON a.id = b.id",
        "SELECT * FROM a CROSS JOIN b",
        "SELECT * FROM c JOIN (a JOIN b USING (id)) ON true",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(SOURCES)));
}

#[test]
fn a_wide_relation_resolves_columns_as_a_narrow_one() {
    // Past 64 columns a relation is indexed by name, which must not change what resolves.
    const INDEXED_WIDTH: usize = 64;
    let ddl = |width: usize| {
        let columns: Vec<String> = (0..width).map(|i| format!("c{i} int")).collect();
        let constants: String = (0..width).map(|i| format!(", 1 AS c{i}")).collect();
        format!(
            "CREATE TABLE w ({}, k text NOT NULL, \"K\" int); CREATE VIEW v AS SELECT * FROM w;
            CREATE VIEW s AS SELECT n.*{constants} FROM nope AS n;",
            columns.join(", ")
        )
    };
    let filler = |width: usize| (0..width).map(|i| format!(", c{i}")).collect::<String>();
    for dialect in ["postgres", "sqlite"] {
        let queries = |width: usize| {
            [
                "WITH c AS (SELECT * FROM w) SELECT k, \"K\", c1, nope FROM c".to_string(),
                "SELECT K, c1 FROM (SELECT * FROM w) AS d".to_string(),
                "SELECT k, \"K\", c1 FROM v".to_string(),
                format!(
                    "SELECT x FROM (SELECT c1 AS x, k AS x{} FROM w) AS d",
                    filler(width)
                ),
                "SELECT bogus FROM s".to_string(),
                format!(
                    "SELECT bogus FROM (SELECT n.*{} FROM w, nope AS n) AS d",
                    filler(width)
                ),
            ]
        };
        for (narrow, wide) in queries(2).iter().zip(queries(INDEXED_WIDTH)) {
            assert_eq!(
                report(narrow, dialect, Some(&ddl(2))),
                report(&wide, dialect, Some(&ddl(INDEXED_WIDTH))),
                "{dialect}: {wide}"
            );
        }
    }
}
