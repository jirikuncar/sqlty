"""Query shapes the analyzer reads rarely: the columns and the name rules it reports."""

import pytest

from sqlty import _core
from sqlty.core import json_shapes

SCHEMA = "CREATE TABLE t (a INT NOT NULL, b TEXT); CREATE VIEW v AS SELECT * FROM nope;"
SHAPES = [
    (
        "postgres",
        "WITH RECURSIVE r AS (TABLE t UNION ALL SELECT a, b FROM r) SELECT * FROM r",
        "a: int, b: str | None",
        [],
    ),
    (
        "postgres",
        "WITH RECURSIVE r(x, y) AS (SELECT 1, NULL::int UNION ALL SELECT y, x FROM r) SELECT * FROM r",
        "x: int | None, y: int | None",
        [],
    ),
    (
        "postgres",
        "WITH RECURSIVE r(x, y, z) AS (SELECT 1, 2, NULL::int UNION ALL SELECT z, x, y FROM r) SELECT * FROM r",
        "x: int | None, y: int | None, z: int | None",
        [],
    ),
    (
        "postgres",
        "WITH RECURSIVE r AS (SELECT 1 AS n UNION SELECT 2 UNION ALL SELECT n + 1 FROM r WHERE n < 5) SELECT * FROM r",
        "n: int",
        [],
    ),
    (
        "postgres",
        "WITH RECURSIVE r AS (SELECT 1 AS n UNION ALL TABLE r) SELECT * FROM r",
        "n: int",
        [],
    ),
    (
        "mysql",
        "WITH RECURSIVE r AS (VALUES ROW(1) UNION ALL SELECT column_0 + 1 FROM r) SELECT * FROM r",
        "column_0: int",
        [],
    ),
    (
        "sqlite",
        "WITH RECURSIVE r AS (VALUES (1) UNION ALL SELECT column1 + 1 FROM r) SELECT * FROM r",
        "column1: int",
        [],
    ),
    ("duckdb", "SELECT a FROM t ORDER BY ALL", "a: int", []),
    ("postgres", "(SELECT a FROM t) UNION SELECT 2", "a: int", []),
    ("databricks", "SELECT stack(1, a, b) AS (x, y) FROM t", "x: None", ["unknown-function"]),
    ("postgres", "SELECT (t).* FROM t", "a: int, b: str | None", []),
    ("postgres", "SELECT * FROM v", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT (NULL), a FROM t", "?column?: None, a: int", []),
    ("postgres", "SELECT * FROM (TABLE t) s", "a: int, b: str | None", []),
    ("postgres", "SELECT * FROM (VALUES (1)) s", "column1: typing.Literal[1]", []),
    (
        "postgres",
        "SELECT * FROM (WITH RECURSIVE r AS (TABLE t UNION ALL SELECT a, b FROM r) SELECT * FROM r) s",
        "a: int, b: str | None",
        [],
    ),
    (
        "postgres",
        "SELECT * FROM (WITH RECURSIVE r(x, y, z) AS (SELECT 1, 2, NULL::int UNION ALL SELECT z, x, y FROM r) SELECT * FROM r) s",
        "x: int | None, y: int | None, z: int | None",
        [],
    ),
    (
        "duckdb",
        "SELECT * FROM (WITH RECURSIVE r AS (VALUES (1) UNION ALL SELECT col0 + 1 FROM r) SELECT * FROM r) s",
        "*: None",
        ["unsupported-syntax"],
    ),
    ("postgres", "SELECT * FROM (SELECT a FROM t ORDER BY a) s ORDER BY a", "a: int", []),
    ("postgres", "SELECT 1 AS a UNION (WITH c AS (SELECT 2 AS a) SELECT a FROM c)", "a: int", []),
    ("postgres", "SELECT a, b FROM t UNION TABLE t", "a: int, b: str | None", []),
    ("duckdb", "SELECT 1 UNION VALUES (2)", "?column?: int", ["unsupported-syntax"]),
    ("postgres", "SELECT foo(a).* FROM t", "*: None", ["unknown-function"]),
    ("postgres", "SELECT (t.a).* FROM t", "*: None", []),
    ("postgres", "SELECT * FROM t CROSS JOIN v", "*: None", ["unsupported-syntax"]),
    ("duckdb", "SELECT 1 AS a, NULL AS b UNION TABLE t", "a: int, b: None", ["unsupported-syntax"]),
    (
        "duckdb",
        "WITH RECURSIVE r AS (TABLE t UNION ALL SELECT a, b FROM r) SELECT * FROM r",
        "*: None",
        ["unsupported-syntax"],
    ),
    ("bigquery", "SELECT STRUCT(1 AS x).* FROM t", "*: None", ["unsupported-syntax"]),
    (
        "postgres",
        "SELECT t.*, v.* FROM t, v",
        "a: int, b: str | None, *: None",
        ["unsupported-syntax"],
    ),
    ("postgres", "SELECT * FROM t, (SELECT * FROM v) s", "*: None", ["unsupported-syntax"]),
    ("postgres", "WITH w AS (SELECT * FROM v) SELECT * FROM w", "*: None", ["unsupported-syntax"]),
    ("postgres", "WITH w AS (DELETE FROM t) SELECT * FROM w", "*: None", []),
    ("postgres", "DELETE FROM v RETURNING *", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT * FROM t JOIN v USING (a)", "*: None", ["unsupported-syntax"]),
]


@pytest.mark.parametrize(("dialect", "sql", "columns", "rules"), SHAPES)
def test_shapes(dialect: str, sql: str, columns: str, rules: list[str]) -> None:
    found = _core.analyze_select(sql, dialect=dialect, schema_sql=SCHEMA)
    assert found is not None
    assert ", ".join(f"{name}: {python}" for name, python, _ in found) == columns
    diagnostics = _core.query_diagnostics(sql, dialect=dialect, schema_sql=SCHEMA)
    assert [rule for rule, *_ in diagnostics] == rules


@pytest.mark.parametrize(
    ("dialect", "sql", "names"),
    [
        ("postgres", "SELECT (t).*, t.* FROM t", "abab"),
        ("bigquery", "SELECT STRUCT(1 AS x).* FROM t", "*"),
    ],
)
def test_explained_stars_have_no_rules(dialect: str, sql: str, names: str) -> None:
    found = _core.explain_select(sql, dialect=dialect, schema_sql=SCHEMA)
    assert found == [(name, None, None) for name in names]


JSON_SCHEMA = (
    "CREATE TABLE t (a INT NOT NULL, j JSONB); CREATE TYPE n AS (child n); CREATE TABLE r (x n);"
)
JSON = [
    (
        "postgres",
        "SELECT jsonb_build_object('a', 1) -> ('a') AS x, jsonb_build_object('a', 1) -> 'a'::text AS y",
        [("x", "integer"), ("y", "integer")],
    ),
    (
        "postgres",
        "SELECT COALESCE((NULL), jsonb_build_object('a', 1)) AS x",
        [("x", "{a: integer}")],
    ),
    (
        "postgres",
        "SELECT jsonb_agg(a) -> (0) AS x, jsonb_agg(a) -> -1 AS y, jsonb_agg(a) -> max(a) AS z FROM t",
        [("x", "integer"), ("y", "integer")],
    ),
    (
        "postgres",
        "SELECT jsonb_build_object('a', 1) #> ('{a}') AS x, jsonb_build_object('a', 1) #> ARRAY['a'] AS y, jsonb_build_object('a', 1) #> '{}' AS z",
        [("x", "integer"), ("y", "integer"), ("z", "{a: integer}")],
    ),
    (
        "postgres",
        "SELECT jsonb_build_object(a::text, 1) -> 'k' AS x, jsonb_agg(a) -> 'k' AS y, jsonb_agg(a) #> '{0}' AS z FROM t GROUP BY a",
        [("x", "integer"), ("z", "integer")],
    ),
    (
        "postgres",
        "SELECT jsonb_agg(u) FILTER (WHERE (u IS NOT NULL AND u.a > 0) AND nope.* IS NOT NULL) AS x FROM t LEFT JOIN t u ON u.a = t.a",
        [("x", "[{a: integer, j: any | null}]")],
    ),
    (
        "postgres",
        "SELECT jsonb_build_object('x', j -> 0) AS x, jsonb_build_object('a', 1) #> '{a,b}' AS y FROM t",
        [("x", "{x: any | null}")],
    ),
    (
        "postgres",
        "SELECT NULLIF(jsonb_build_object('a', 1), '{}') -> 'a' AS x, jsonb_build_object('a') AS y",
        [("x", "integer")],
    ),
    ("mysql", "SELECT JSON_OBJECT('a', a > 0) AS x FROM t", [("x", "{a: any}")]),
    ("postgres", "WITH w AS (DELETE FROM t) SELECT to_jsonb(w) AS x FROM w", []),
    (
        "postgres",
        "WITH w AS (SELECT jsonb_build_object('a', 1) AS d) SELECT to_jsonb(w) AS x FROM w",
        [("x", "{d: {a: integer}}")],
    ),
    (
        "postgres",
        "SELECT to_jsonb(r) AS x FROM r",
        [
            (
                "x",
                "{x: {child: {child: {child: {child: {child: {child: {child: {child: {child: any | null} | null} | null} | null} | null} | null} | null} | null} | null} | null}",
            )
        ],
    ),
]


@pytest.mark.parametrize(("dialect", "sql", "shapes"), JSON)
def test_json(dialect: str, sql: str, shapes: list[tuple[str, str]]) -> None:
    assert json_shapes(sql, dialect=dialect, schema_sql=JSON_SCHEMA) == shapes


EXPRESSION_SCHEMAS = {
    "postgres": "CREATE TABLE f (i INT, tx TEXT, iv INTERVAL, dt DATE, vb VARBIT, c INT CHECK (c IN (1, 2))); CREATE TABLE p (id INT PRIMARY KEY, name TEXT NOT NULL); CREATE TABLE c (id INT PRIMARY KEY, pid INT REFERENCES p (id), arr INT[]); CREATE TABLE g (a INT, b INT, CHECK (num_nonnulls(a, b) >= 1));",
    "mysql": "CREATE TABLE f (i INT, tx TEXT, d DECIMAL(30, 2), dd DECIMAL(20), dn DECIMAL, dtm DATETIME, c INT CHECK (c IN (1, 2)));",
    "sqlite": "CREATE TABLE f (i INT, tx TEXT, c INT CHECK (c IN (1, 2)));",
}
EXPRESSIONS = [
    (
        "mysql",
        "SELECT GREATEST(c, tx) AS x, GREATEST(i, tx) AS y, GREATEST(tx, i) AS z FROM f",
        "x: str | None, y: str | None, z: str | None",
        [],
    ),
    (
        "sqlite",
        "SELECT coalesce(c, tx) AS x, coalesce(i, tx) AS y FROM f",
        "x: int | str | None, y: int | str | None",
        [],
    ),
    ("mysql", "SELECT STR_TO_DATE(tx, 5) AS x FROM f", "x: None", ["unsupported-syntax"]),
    ("postgres", "SELECT RANGE_MERGE(i) AS x FROM f", "x: None", ["unsupported-syntax"]),
    ("postgres", "SELECT STDDEV(iv) AS x FROM f", "x: datetime.timedelta | None", []),
    ("postgres", "SELECT SQRT(tx) AS x FROM f", "x: None", ["unsupported-syntax"]),
    ("mysql", "SELECT DATE_ADD(dtm, 5) AS x FROM f", "x: datetime.datetime | None", []),
    (
        "mysql",
        "SELECT FLOOR(d) AS x, FLOOR(dd) AS y, FLOOR(dn) AS z, FLOOR(NULL) AS w FROM f",
        "x: decimal.Decimal | None, y: decimal.Decimal | None, z: int | None, w: None",
        ["unsupported-syntax"],
    ),
    ("postgres", "SELECT TIMEZONE('UTC', dt) AS x FROM f", "x: None", ["unsupported-syntax"]),
    ("postgres", "SELECT DATE_TRUNC('day', tx) AS x FROM f", "x: None", ["unsupported-syntax"]),
    ("postgres", "SELECT LTRIM(vb) AS x FROM f", "x: str | None", []),
    (
        "postgres",
        "SELECT RANDOM(1.5::float8, 2.5::float8) AS x FROM f",
        "x: None",
        ["unsupported-syntax"],
    ),
    ("postgres", "SELECT COALESCE(i, tx) AS x FROM f", "x: int | None", []),
    ("postgres", "SELECT COALESCE(c, tx) AS x FROM f", "x: int | None", []),
    ("mysql", "SELECT CONCAT(tx, i) AS x FROM f", "x: str | None", []),
    ("postgres", "SELECT (SELECT 1) AS x FROM c LEFT JOIN p ON p.id = c.pid", "x: int", []),
    (
        "postgres",
        "SELECT p.name AS x FROM c LEFT JOIN p ON c.pid = c.id",
        "x: str | None",
        ["entity-mismatch"],
    ),
    (
        "postgres",
        "SELECT (SELECT DISTINCT p.name FROM p WHERE p.id = c.pid) AS x FROM c",
        "x: str | None",
        [],
    ),
    (
        "postgres",
        "WITH w AS (SELECT * FROM p) SELECT (SELECT w.name FROM w WHERE w.id = c.pid) AS x FROM c",
        "x: str | None",
        [],
    ),
    (
        "postgres",
        "SELECT (SELECT p.name FROM p WHERE p.id = p.id) AS x FROM c",
        "x: str | None",
        [],
    ),
    (
        "postgres",
        "SELECT (SELECT p.name FROM p WHERE ((p.id) = c.pid)) AS x FROM c",
        "x: str | None",
        [],
    ),
    (
        "postgres",
        "SELECT (SELECT p.name FROM p WHERE public.p.id = c.pid) AS x FROM c",
        "x: str | None",
        [],
    ),
    (
        "postgres",
        "SELECT o.n AS x FROM c OUTER APPLY (SELECT count(*) AS n FROM p) o",
        "x: int",
        [],
    ),
    (
        "postgres",
        "SELECT o.n AS x FROM c LEFT JOIN (SELECT count(*) AS n FROM p) o ON (TRUE)",
        "x: int",
        [],
    ),
    (
        "postgres",
        "SELECT c.id / (1) AS x, c.id / 'a' AS y FROM c",
        "x: int, y: None",
        ["unsupported-syntax"],
    ),
    ("postgres", "SELECT c.id + 1 AS x FROM c GROUP BY ROLLUP (c.id + 1)", "x: int | None", []),
    ("postgres", "SELECT c.id AS x FROM c WHERE c.id = ANY ((ARRAY[1, 2]))", "x: int", []),
    (
        "postgres",
        "SELECT c.id = ANY (c.arr) AS x, c.id = ANY ((ARRAY[1, 2])) AS y FROM c",
        "x: bool | None, y: bool",
        [],
    ),
    (
        "postgres",
        "SELECT c.id = ANY ((SELECT * FROM nope)) AS x FROM c",
        "x: bool",
        ["unknown-table"],
    ),
    (
        "postgres",
        "SELECT sum(c.id) OVER w AS x FROM c WINDOW w AS (ORDER BY c.id)",
        "x: int | None",
        [],
    ),
    ("postgres", "SELECT NVL(a, b) AS x FROM g", "x: None", ["unknown-function"]),
]


@pytest.mark.parametrize(("dialect", "sql", "columns", "rules"), EXPRESSIONS)
def test_expressions(dialect: str, sql: str, columns: str, rules: list[str]) -> None:
    schema = EXPRESSION_SCHEMAS[dialect]
    found = _core.analyze_select(sql, dialect=dialect, schema_sql=schema)
    assert found is not None
    assert ", ".join(f"{name}: {python}" for name, python, _ in found) == columns
    diagnostics = _core.query_diagnostics(sql, dialect=dialect, schema_sql=schema)
    assert [rule for rule, *_ in diagnostics] == rules
