//! Function names through `sqlty_sql::highlight`.

use sqlty_sql::highlight::{highlight, Highlight};

fn functions<'a>(sql: &'a str, dialect: &str) -> Vec<&'a str> {
    highlight(sql, dialect)
        .into_iter()
        .filter(|(_, kind)| *kind == Highlight::Function)
        .map(|(range, _)| &sql[range])
        .collect()
}

#[test]
fn a_word_called_is_a_function() {
    for dialect in ["postgres", "mysql", "sqlite", "duckdb"] {
        assert_eq!(
            functions(
                "SELECT count(*), coalesce(name, ''), count FROM t WHERE created < now()",
                dialect
            ),
            ["count", "coalesce", "now"],
            "{dialect}"
        );
    }
    assert_eq!(
        functions(
            "SELECT pg_catalog.now(), \"my fn\"(1) FROM t WHERE id IN (1) AND EXISTS (SELECT 1)",
            "postgres"
        ),
        ["now", "\"my fn\""]
    );
    assert_eq!(
        functions(
            "WITH c(a) AS (SELECT 1) INSERT INTO public.t(a) SELECT CAST(a AS numeric(10, 2)), \
             a::varchar(3), count (a) FROM c AS x(a) JOIN u USING (a) VALUES (1)",
            "postgres"
        ),
        Vec::<&str>::new()
    );
    for sql in [
        "WITH a AS (SELECT 1), b(x) AS (SELECT 2) SELECT x FROM b",
        "SELECT n FROM generate_series(1, 3) g(n)",
        "SELECT id FROM unnest(%(ids)s::int[]) t(id)",
        "CREATE TABLE IF NOT EXISTS t(a varchar(3), b numeric(10, 2))",
        "COPY users(id) FROM STDIN",
        "INSERT INTO /* c */ users(id) VALUES (1)",
    ] {
        assert!(
            functions(sql, "postgres")
                .iter()
                .all(|name| ["generate_series", "unnest"].contains(name)),
            "{sql}"
        );
    }
    assert_eq!(
        functions("SELECT count(*) AS n, lower(x) FROM t", "postgres"),
        ["count", "lower"]
    );
}
