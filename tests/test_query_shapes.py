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


@pytest.mark.parametrize("sql", ["VALUES (1)", "SELECT -nope", "SELECT nofunc(1)"])
def test_unexplained_columns_have_no_rules(sql: str) -> None:
    found = _core.explain_select(sql, dialect="postgres", schema_sql=SCHEMA)
    assert found is not None
    assert [rules for _, *rules in found] == [[None, None]]


def test_a_parenthesized_query_is_explained() -> None:
    found = _core.explain_select("(SELECT count(*) FROM t)", dialect="postgres", schema_sql=SCHEMA)
    assert found is not None
    assert [
        (name, type_rule[0], nullability_rule[0]) for name, type_rule, nullability_rule in found
    ] == [("count", "common.aggregate.count", "common.aggregate.count")]


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
    "postgres": "CREATE TABLE f (i INT, tx TEXT, iv INTERVAL, dt DATE, vb VARBIT, c INT CHECK (c IN (1, 2))); CREATE TABLE p (id INT PRIMARY KEY, name TEXT NOT NULL); CREATE TABLE c (id INT PRIMARY KEY, pid INT REFERENCES p (id), arr INT[]); CREATE TABLE g (a INT, b INT, CHECK (num_nonnulls(a, b) >= 1)); CREATE TABLE vs (x INT CHECK (x IN (1, 2)), y INT CHECK (y IN (3, 4))); CREATE VIEW bad AS SELECT * FROM nope; CREATE TYPE mood AS ENUM ('a'); CREATE TABLE mt (m mood); ALTER TYPE mood RENAME TO feeling; DROP TYPE feeling; CREATE TYPE pair AS (a INT, b TEXT); CREATE TABLE pt (p pair); ALTER TYPE pair RENAME TO duo; CREATE TABLE gt (a INT, b INT GENERATED ALWAYS AS (a + 1) STORED, c INT GENERATED ALWAYS AS (gt.b + 1) STORED); CREATE VIEW pv AS SELECT id FROM p; CREATE FUNCTION fv() RETURNS SETOF pv AS $$ SELECT * FROM pv $$ LANGUAGE sql; CREATE TABLE ut (a INT, b mystery); CREATE VIEW v2 AS SELECT x.id FROM p x, c x; CREATE TYPE pr AS (a INT); CREATE TABLE ptt (q pr); CREATE VIEW v3 AS TABLE p; CREATE TABLE pk2 (x INT, y INT, PRIMARY KEY (x, y));",
    "mysql": "CREATE TABLE f (i INT, tx TEXT, d DECIMAL(30, 2), dd DECIMAL(20), dn DECIMAL, dtm DATETIME, c INT CHECK (c IN (1, 2))); CREATE TABLE m (ui INT UNSIGNED, ub BIGINT UNSIGNED, y YEAR, g GEOMETRY, dt DATE, bl BLOB, ud DECIMAL(10, 2) UNSIGNED, j JSON); CREATE TABLE u (UserName TEXT);",
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
    (
        "mysql",
        "SELECT i + INTERVAL 1 DAY FROM f",
        "i + INTERVAL 1 DAY: None",
        ["unsupported-syntax"],
    ),
    (
        "mysql",
        "SELECT INTERVAL 1 DAY - dt FROM m",
        "INTERVAL 1 DAY - dt: None",
        ["unsupported-syntax"],
    ),
    (
        "mysql",
        "SELECT ud + 1, ud % 2, ud / 2, ud | 1 FROM m",
        "ud + 1: decimal.Decimal | None, ud % 2: decimal.Decimal | None, ud / 2: decimal.Decimal | None, ud | 1: int | None",
        [],
    ),
    (
        "mysql",
        "SELECT CAST(1 AS UNSIGNED) * 1.5 FROM m",
        "CAST(1 AS UNSIGNED) * 1.5: decimal.Decimal",
        [],
    ),
    (
        "mysql",
        "SELECT -g, ~dt, -ui, -ud FROM m",
        "-g: bytes | None, ~dt: None, -ui: int | None, -ud: decimal.Decimal | None",
        ["unsupported-syntax"],
    ),
    (
        "mysql",
        "SELECT CASE WHEN true THEN g ELSE 1 END FROM m",
        "CASE WHEN true THEN g ELSE 1 END: bytes | None",
        [],
    ),
    (
        "mysql",
        "SELECT CASE WHEN true THEN ui ELSE ub END FROM m",
        "CASE WHEN true THEN ui ELSE ub END: int | None",
        [],
    ),
    (
        "mysql",
        "SELECT CASE WHEN true THEN i ELSE ub END FROM m, f",
        "CASE WHEN true THEN i ELSE ub END: decimal.Decimal | None",
        [],
    ),
    (
        "mysql",
        "SELECT CASE WHEN true THEN ub ELSE i END FROM m, f",
        "CASE WHEN true THEN ub ELSE i END: decimal.Decimal | None",
        [],
    ),
    (
        "mysql",
        "SELECT CASE WHEN true THEN y ELSE 1 END FROM m",
        "CASE WHEN true THEN y ELSE 1 END: int | None",
        [],
    ),
    (
        "mysql",
        "SELECT CASE WHEN true THEN j ELSE bl END FROM m",
        "CASE WHEN true THEN j ELSE bl END: bytes | None",
        [],
    ),
    (
        "mysql",
        "SELECT CASE WHEN true THEN j ELSE dt END FROM m",
        "CASE WHEN true THEN j ELSE dt END: str | None",
        [],
    ),
    (
        "postgres",
        "SELECT |/ 4.0, @ -1, ~ 1, - i, + i FROM f",
        "?column?: float, ?column?: int, ?column?: int, ?column?: int | None, ?column?: int | None",
        [],
    ),
    (
        "sqlite",
        "SELECT CASE WHEN 1 THEN i WHEN 2 THEN tx ELSE i END FROM f",
        "CASE WHEN 1 THEN i WHEN 2 THEN tx ELSE i END: int | str | None",
        [],
    ),
    ("mysql", "SELECT !i, - tx FROM f", "!i: bool | None, - tx: float | None", []),
    (
        "postgres",
        "SELECT - iv, @ i FROM f",
        "?column?: datetime.timedelta | None, ?column?: int | None",
        [],
    ),
    ("postgres", "SELECT i ! FROM f", "?column?: None", ["unsupported-syntax"]),
    ("postgres", "SELECT s FROM (SELECT 1) s", "s: None", []),
    ("postgres", "SELECT i FROM f WHERE c = 1", "i: int | None", []),
    ("postgres", "SELECT y FROM vs WHERE x = 1", "y: typing.Literal[3, 4] | None", []),
    ("postgres", "SELECT excluded.i FROM f", "i: None", []),
    ("mysql", "SELECT Usernme FROM u", "Usernme: None", ["unknown-column"]),
    ("postgres", "SELECT * FROM bad", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT bad.* FROM bad", "*: None", ["unsupported-syntax"]),
    (
        "postgres",
        "SELECT * FROM bad, (SELECT * FROM nope) n",
        "*: None",
        ["unknown-table", "unsupported-syntax"],
    ),
    ("postgres", "SELECT * FROM bad CROSS JOIN LATERAL f()", "*: None", ["unsupported-syntax"]),
    ("mysql", "SELECT 1e3, 99999999999999999999", "1e3: float, 99999999999999999999: int", []),
    ("sqlite", "SELECT 1e3, 99999999999999999999", "1e3: float, 99999999999999999999: int", []),
    ("mysql", "SELECT 'a'::ENUM('A', 'B')", "'a'::ENUM('A', 'B'): str", []),
    ("mysql", "SELECT CAST(i AS a.b) FROM f", "CAST(i AS a.b): None", ["unknown-type"]),
    ("postgres", "SELECT CAST(i AS a.b) FROM f", "i: None", ["unknown-type"]),
    (
        "postgres",
        "SELECT (p).name, (p).*, (p).nope FROM p",
        "name: str, id: int, name: str, nope: None",
        ["unknown-column"],
    ),
    ("postgres", "SELECT CASE WHEN true THEN c ELSE i END FROM f", "i: int | None", []),
    (
        "postgres",
        "SELECT MIN(ARRAY[]), MAX(ARRAY[i]) FROM f",
        "min: None, max: list[int] | None",
        ["unsupported-syntax"],
    ),
    (
        "postgres",
        "SELECT ARRAY[(1, 2)], (ROW(1, 2)).f1, ('(1,2)'::nope).x",
        "array: list[tuple[typing.Any, ...]], f1: None, x: None",
        ["unsupported-syntax", "unknown-type"],
    ),
    (
        "postgres",
        "SELECT CAST(NULL AS INT ARRAY)[1], CAST(NULL AS p[])",
        "int array: int | None, p: list | None",
        [],
    ),
    ("postgres", "SELECT j.* FROM (f JOIN p ON true) AS j", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT * FROM (f JOIN p ON true) AS j", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT g.* FROM f, LATERAL generate_series(1, 2) AS g", "g: int", []),
    (
        "postgres",
        "SELECT * FROM f, LATERAL generate_series(1, 2) AS g",
        "i: int | None, tx: str | None, iv: datetime.timedelta | None, dt: datetime.date | None, vb: str | None, c: typing.Literal[1, 2] | None, g: int",
        [],
    ),
    ("postgres", "SELECT *", "*: None", []),
    ("postgres", "SELECT nope.*", "*: None", ["unknown-table"]),
    (
        "mysql",
        "SELECT * FROM JSON_TABLE('[]', '$[*]' COLUMNS (a INT PATH '$.a', NESTED PATH '$.b[*]' COLUMNS (b INT PATH '$'))) AS jt",
        "*: None",
        ["unsupported-syntax"],
    ),
    (
        "mysql",
        "SELECT * FROM JSON_TABLE('[]', '$[*]' COLUMNS (a INT PATH '$.a')) AS jt JOIN f ON true",
        "a: int | None, i: int | None, tx: str | None, d: decimal.Decimal | None, dd: decimal.Decimal | None, dn: decimal.Decimal | None, dtm: datetime.datetime | None, c: int | None",
        [],
    ),
    (
        "postgres",
        "SELECT * FROM p FULL JOIN p AS q USING (id) WHERE p.id IS NOT NULL AND q.id IS NOT NULL",
        "id: int, name: str, name: str",
        [],
    ),
    ("postgres", "SELECT * FROM p JOIN p AS q USING (p.id)", "id: int, name: str, name: str", []),
    (
        "postgres",
        "SELECT * FROM p NATURAL JOIN c, f",
        "id: int, name: str, pid: int | None, arr: list[int] | None, i: int | None, tx: str | None, iv: datetime.timedelta | None, dt: datetime.date | None, vb: str | None, c: typing.Literal[1, 2] | None",
        [],
    ),
    (
        "postgres",
        "SELECT * FROM p JOIN p AS q USING (id) CROSS JOIN g",
        "id: int, name: str, name: str, a: int | None, b: int | None",
        [],
    ),
    ("postgres", "SELECT * FROM p JOIN c USING (nope)", "*: None", ["unsupported-syntax"]),
    (
        "postgres",
        "SELECT i FROM f WHERE (i IS NULL AND i IS NOT NULL) OR (tx IS NULL AND tx IS NOT NULL)",
        "i: int | None",
        [],
    ),
    ("postgres", "SELECT i FROM f WHERE NULL IS DISTINCT FROM i", "i: int", []),
    (
        "postgres",
        "SELECT CASE WHEN (c = 1) THEN 0 ELSE c END AS x, CASE WHEN 1 = c THEN 0 ELSE c END AS y FROM f",
        "x: typing.Literal[0, 2] | None, y: typing.Literal[0, 2] | None",
        [],
    ),
    (
        "postgres",
        "SELECT i FROM f WHERE EXISTS (SELECT 1 FROM g WHERE g.a = f.i UNION SELECT 1)",
        "i: int | None",
        [],
    ),
    (
        "postgres",
        "SELECT c FROM f WHERE EXISTS (SELECT 1 FROM g WHERE f.c = 1 AND g.a = f.i)",
        "c: typing.Literal[1]",
        [],
    ),
    ("postgres", "SELECT m FROM mt", "m: None", ["unknown-type"]),
    ("postgres", "SELECT (p).a, (p).b FROM pt", "a: int | None, b: str | None", []),
    ("postgres", "SELECT c FROM gt", "c: int | None", []),
    ("postgres", "SELECT * FROM fv()", "id: int", []),
    ("postgres", "SELECT unnest(ARRAY[ARRAY[1, 2]]) AS x", "x: int | None", []),
    ("mysql", "SELECT db.lower('a') AS x", "x: None", ["unknown-function"]),
    ("postgres", "SELECT CASE WHEN i > 0 THEN 1 ELSE dt END AS x FROM f", "x: int | None", []),
    (
        "postgres",
        "SELECT percentile_disc(ARRAY[0.5]) WITHIN GROUP (ORDER BY nope) AS x FROM f",
        "x: None",
        ["unknown-column"],
    ),
    ("sqlite", "SELECT * FROM pragma_table_info('f')", "*: None", ["unsupported-syntax"]),
    ("mysql", "SELECT * FROM generate_series(1, 2)", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT value FROM jsonb_array_elements('[1]')", "value: typing.Any", []),
    ("postgres", "SELECT key, value FROM json_each('{}')", "key: str, value: typing.Any", []),
    (
        "postgres",
        "SELECT * FROM jsonb_populate_record(NULL, '{}')",
        "*: None",
        ["unsupported-syntax"],
    ),
    ("postgres", "SELECT i FROM f GROUP BY 5", "i: int | None", ["ungrouped-column"]),
    ("postgres", "SELECT * FROM (SELECT 1 AS a) s GROUP BY a", "a: int", []),
    ("postgres", "WITH w AS (SELECT 1 AS a) SELECT * FROM w GROUP BY a", "a: int", []),
    ("postgres", "SELECT w.* FROM (SELECT 1 AS a) w GROUP BY w.a", "a: int", []),
    (
        "postgres",
        "SELECT p.*, c.* FROM p JOIN c ON c.pid = p.id GROUP BY p.id",
        "id: int, name: str, id: int, pid: int, arr: list[int] | None",
        ["ungrouped-column", "ungrouped-column", "ungrouped-column"],
    ),
    ("postgres", "SELECT s FROM (SELECT 1 AS a) s", "s: None", []),
    ("postgres", "SELECT i FROM f WHERE c = 1", "i: int | None", []),
    ("mysql", "SELECT usernme FROM u", "usernme: None", ["unknown-column"]),
    ("postgres", "SELECT * FROM bad", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT * FROM ut", "a: int | None, b: None", ["unknown-type"]),
    (
        "postgres",
        "SELECT * FROM f, LATERAL generate_series(1, i)",
        "i: int | None, tx: str | None, iv: datetime.timedelta | None, dt: datetime.date | None, vb: str | None, c: typing.Literal[1, 2] | None, generate_series: int",
        [],
    ),
    ("postgres", "SELECT x.* FROM f", "*: None", ["unknown-table"]),
    ("postgres", "SELECT *", "*: None", []),
    (
        "postgres",
        "WITH w AS (SELECT DISTINCT i FROM f) SELECT * FROM (WITH w AS (SELECT i FROM f) SELECT * FROM w) x",
        "i: int | None",
        [],
    ),
    ("postgres", "SELECT 1 AS x FROM c WHERE c.pid IN (SELECT 1 UNION SELECT 2)", "x: int", []),
    ("postgres", "SELECT 1 AS x FROM c WHERE c.pid IN (SELECT * FROM p)", "x: int", []),
    (
        "mysql",
        "SELECT ADDDATE(dt, 1) AS a, ADDDATE(dtm, 1) AS b, ADDDATE(ui, 1) AS c FROM f, m",
        "a: datetime.date | None, b: datetime.datetime | None, c: None",
        ["unsupported-syntax"],
    ),
    ("postgres", "SELECT i / (2) AS a, i / NULL AS b FROM f", "a: int | None, b: int | None", []),
    (
        "postgres",
        "SELECT 0xffffffffffffffffffffffffffffffffffffffff AS a",
        "a: decimal.Decimal",
        [],
    ),
    ("sqlite", "SELECT 0xFFFFFFFFFFFFFFFFFF AS a, 0x10 AS b", "a: bytes, b: int", []),
    ("postgres", "SELECT * FROM v2", "id: int", []),
    ("postgres", "(SELECT * FROM bad)", "*: None", ["unsupported-syntax"]),
    ("mysql", "SELECT unnest(j) FROM m", "unnest(j): None", ["unknown-function"]),
    ("postgres", "SELECT (pt.p).* FROM pt", "a: int | None, b: str | None", []),
    ("postgres", "SELECT CAST(NULL AS p[])", "p: list | None", []),
    ("postgres", "SELECT CAST(NULL AS pair[])", "pair: list | None", []),
    ("postgres", "SELECT (c.arr)[1].x FROM c", "x: None", ["unsupported-syntax"]),
    ("postgres", "SELECT (pt.p).a.b FROM pt", "b: None", ["unknown-type"]),
    ("postgres", "SELECT NULLIF(nope, 1) FROM f", "nullif: None", ["unknown-column"]),
    ("postgres", "SELECT NULLIF(NULL, 1)", "nullif: None", ["unsupported-syntax"]),
    ("mysql", "SELECT DATE_ADD(dtm) FROM f", "DATE_ADD(dtm): None", ["unsupported-syntax"]),
    (
        "mysql",
        "SELECT FLOOR(d), CEIL(d), ROUND(d), TRUNCATE(d, 0) FROM f",
        "FLOOR(d): decimal.Decimal | None, CEIL(d): decimal.Decimal | None, ROUND(d): decimal.Decimal | None, TRUNCATE(d, 0): decimal.Decimal | None",
        [],
    ),
    ("postgres", "SELECT * FROM (SELECT 1 AS a) s GROUP BY a", "a: int", []),
    ("postgres", "SELECT bad FROM bad GROUP BY 1", "bad: None", []),
    ("postgres", "SELECT (SELECT x.id FROM p x, c x)", "id: int | None", ["duplicate-alias"]),
    ("mysql", "SELECT i / (2) FROM f", "i / (2): decimal.Decimal | None", []),
    ("mysql", "SELECT i / '2' FROM f", "i / '2': float | None", []),
    ("postgres", "SELECT NVL(tx, tx) FROM f", "nvl: None", ["unknown-function"]),
    ("mysql", "SELECT COALESCE(y, ui) FROM m", "COALESCE(y, ui): int | None", []),
    (
        "mysql",
        "SELECT COALESCE(i, CAST(1 AS SIGNED)) FROM f",
        "COALESCE(i, CAST(1 AS SIGNED)): int",
        [],
    ),
    (
        "mysql",
        "SELECT CASE WHEN TRUE THEN i ELSE y END FROM f, m",
        "CASE WHEN TRUE THEN i ELSE y END: int | None",
        [],
    ),
    ("postgres", "SELECT 0x1F", "?column?: int", []),
    ("postgres", "SELECT s FROM (SELECT 1 AS a) s", "s: None", []),
    ("postgres", "SELECT x.c FROM f x WHERE x.c = 1", "c: typing.Literal[1]", []),
    ("postgres", "SELECT (c) FROM f WHERE c = 1", "c: typing.Literal[1]", []),
    ("postgres", "SELECT f.c FROM f WHERE c = 1", "c: typing.Literal[1]", []),
    ("mysql", "SELECT UserNam FROM u", "UserNam: None", ["unknown-column"]),
    ("postgres", "SELECT nope.* FROM p", "*: None", ["unknown-table"]),
    ("postgres", "SELECT *", "*: None", []),
    ("postgres", "SELECT CAST(NULL AS BINARY LARGE OBJECT)", "blob: bytes | None", []),
    ("postgres", "SELECT * FROM json_array_elements('[1]')", "value: typing.Any", []),
    ("postgres", "SELECT MOD(i, 2) FROM f", "mod: int | None", []),
    ("mysql", "SELECT MOD(i, (2)) FROM f", "MOD(i, (2)): int | None", []),
    ("sqlite", "SELECT LOCALTIME", "LOCALTIME: None", ["unknown-function"]),
    (
        "postgres",
        "SELECT * FROM p, (SELECT 1) GROUP BY p.id",
        "id: int, name: str, ?column?: int",
        ["ungrouped-column"],
    ),
    ("postgres", 'SELECT 1 FROM ptt WHERE (ptt.q)."*" IS NULL', "?column?: int", []),
    (
        "mysql",
        "WITH c AS (SELECT 1 AS UserName) SELECT UserNam FROM c",
        "UserNam: None",
        ["unknown-column"],
    ),
    ("postgres", "SELECT nope FROM p GROUP BY id", "nope: None", ["unknown-column"]),
    (
        "postgres",
        "SELECT * FROM p, pg_catalog.pg_nothing GROUP BY p.id",
        "*: None",
        ["unsupported-syntax"],
    ),
    (
        "postgres",
        "SELECT * FROM p, (c JOIN f ON true) AS j GROUP BY p.id",
        "*: None",
        ["unsupported-syntax"],
    ),
    (
        "postgres",
        "SELECT * FROM p, unnest(ARRAY[1]) GROUP BY p.id",
        "id: int, name: str, unnest: int | None",
        ["ungrouped-column"],
    ),
    ("postgres", "SELECT * FROM p, pg_nothing() GROUP BY p.id", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT * FROM v3", "*: None", ["unsupported-syntax"]),
    ("postgres", "SELECT (pt.p).f(1) FROM pt", "p: None", ["unknown-type"]),
    ("postgres", "SELECT 1 FROM pk2 WHERE pk2.x = pk2.y", "?column?: int", ["entity-mismatch"]),
    ("postgres", "SELECT x FROM pg_catalog.pg_nothing x", "x: None", []),
    (
        "postgres",
        'SELECT coalesce((ptt.q)."*", 1) AS n FROM ptt',
        "n: int",
        ["unknown-type"],
    ),
]


@pytest.mark.parametrize(("dialect", "sql", "columns", "rules"), EXPRESSIONS)
def test_expressions(dialect: str, sql: str, columns: str, rules: list[str]) -> None:
    schema = EXPRESSION_SCHEMAS[dialect]
    found = _core.analyze_select(sql, dialect=dialect, schema_sql=schema)
    assert found is not None
    assert ", ".join(f"{name}: {python}" for name, python, _ in found) == columns
    diagnostics = _core.query_diagnostics(sql, dialect=dialect, schema_sql=schema)
    assert [rule for rule, *_ in diagnostics] == rules


STATEMENT_SCHEMAS = {
    "postgres": "CREATE TABLE t (a INT NOT NULL, b TEXT, c INT NOT NULL DEFAULT 1); CREATE TABLE u (a INT, b TEXT); CREATE FUNCTION f() RETURNS TRIGGER AS $$ BEGIN RETURN NEW; END $$ LANGUAGE plpgsql; CREATE TABLE trg (a INT); CREATE TRIGGER x BEFORE UPDATE ON trg FOR EACH ROW EXECUTE FUNCTION f();",
    "mysql": "CREATE TABLE t (a INT NOT NULL, b TEXT, c INT NOT NULL DEFAULT 1); CREATE TABLE u (a INT, b TEXT);",
    "sqlite": "CREATE TABLE t (a INT NOT NULL, b TEXT, c INT NOT NULL DEFAULT 1); CREATE TABLE u (a INT, b TEXT);",
    "clickhouse": "CREATE TABLE t (a INT NOT NULL, b TEXT);",
}
STATEMENTS = [
    ("postgres", "UPDATE t SET (a, b) = (1, 'x') RETURNING a, b", "a: int, b: str | None", []),
    ("postgres", "UPDATE t SET (a, nope) = (1, 'x')", None, ["unknown-column"]),
    (
        "postgres",
        "INSERT INTO t (a) VALUES (1) ON CONFLICT (a) DO UPDATE SET (b, c) = ('x', 2) RETURNING a, b",
        "a: int, b: str | None",
        [],
    ),
    ("postgres", "INSERT INTO t (a) VALUES (1) ON CONFLICT DO NOTHING", None, []),
    (
        "postgres",
        "MERGE INTO t USING u ON t.a = u.a WHEN MATCHED THEN UPDATE SET (a, b) = (1, 'x')",
        None,
        [],
    ),
    ("mysql", "UPDATE t JOIN u ON t.a = u.a SET (t.a, u.b) = (1, 'x')", None, []),
    (
        "mysql",
        "INSERT INTO t VALUES (1, 'x', 2) ON DUPLICATE KEY UPDATE a = 1 RETURNING a",
        "a: int",
        [],
    ),
    (
        "sqlite",
        "INSERT INTO t VALUES (1, 'x', 2) ON CONFLICT DO UPDATE SET (a, b) = (2, 'y') RETURNING a",
        "a: int",
        [],
    ),
    ("postgres", "INSERT INTO t SELECT * FROM nope", None, ["unknown-table"]),
    ("postgres", "INSERT INTO nope VALUES (1)", None, ["unknown-table"]),
    ("postgres", "UPDATE ONLY t SET a = 1 RETURNING a", "a: int", []),
    ("postgres", "UPDATE trg SET a = 1 RETURNING a", "a: int | None", []),
    ("postgres", "UPDATE t* SET a = 1 RETURNING a", "a: int", []),
    ("postgres", "DELETE FROM t USING u WHERE t.a = u.a RETURNING t.a", "a: int", []),
    ("mysql", "INSERT INTO t SET (a, b) = (1, 'x')", None, []),
    ("mysql", "UPDATE (SELECT 1 AS a) AS x SET a = 1", None, []),
    ("clickhouse", "INSERT INTO TABLE FUNCTION remote('x', t) (a) SELECT a FROM t", None, []),
    (
        "clickhouse",
        "INSERT INTO TABLE FUNCTION remote('x', t) (a) SELECT a FROM t RETURNING a",
        "a: None",
        [],
    ),
]


@pytest.mark.parametrize(("dialect", "sql", "columns", "rules"), STATEMENTS)
def test_statements(dialect: str, sql: str, columns: str | None, rules: list[str]) -> None:
    schema = STATEMENT_SCHEMAS[dialect]
    found = _core.analyze_select(sql, dialect=dialect, schema_sql=schema)
    assert (found and ", ".join(f"{name}: {python}" for name, python, _ in found)) == columns
    diagnostics = _core.query_diagnostics(sql, dialect=dialect, schema_sql=schema)
    assert [rule for rule, *_ in diagnostics] == rules


PARAMS_SCHEMA = (
    "CREATE TABLE f (i INT, tx TEXT, arr INT[], d DATE); CREATE TABLE g (a INT NOT NULL);"
)
PARAMS = [
    ("SELECT 1 FROM f WHERE $1 LIKE ANY(ARRAY['a'])", [(1, "str", None)]),
    ("SELECT 1 FROM f WHERE tx ~ ANY($1)", [(1, "list[str]", None)]),
    ("SELECT 1 FROM f WHERE ANY($1) = i", [(1, "list[int]", None)]),
    ("SELECT 1 FROM f WHERE arr @> $1", [(1, "list[int]", None)]),
    ("SELECT 1 FROM f WHERE $1 && arr", [(1, "list[int]", None)]),
    ("SELECT $1 || arr FROM f", [(1, "list[int]", None)]),
    ("SELECT * FROM (SELECT 1 FROM nope) s WHERE $1 = 1", [(1, "int", None)]),
    (
        "INSERT INTO g VALUES ($1), ($2, $3)",
        [(1, "int", False), (2, "int", False), (3, None, None)],
    ),
    ("SELECT 1 FROM f WHERE i = ALL(ARRAY[$1])", [(1, "int", None)]),
    ("SELECT 1 FROM f WHERE tx LIKE ALL($1)", [(1, "list[str]", None)]),
    ("SELECT 1 FROM nope WHERE a = $1", [(1, None, None)]),
    ("VALUES ($1)", [(1, None, None)]),
    ("SELECT 1 FROM f WHERE ANY(arr) = $1", [(1, None, None)]),
    ("SELECT 1 FROM f WHERE $1 = ANY(i)", [(1, None, None)]),
    ("SELECT 1 FROM f WHERE $1 IN (SELECT NULL)", [(1, None, None)]),
    ("SELECT 1 FROM f x, g x WHERE x.i = $1", [(1, "int", None)]),
    ("SELECT * FROM (TABLE g) s WHERE s.a = $1", [(1, "int", None)]),
    ("SELECT 1 FROM f WHERE $1 = ANY(nope)", [(1, None, None)]),
    ("SELECT 1 FROM f WHERE $1 IN (SELECT x.i FROM f x, g x)", [(1, "int", None)]),
    ("TABLE g LIMIT $1", [(1, "int", None)]),
    ("SELECT 1 FROM f WHERE $1 IN (SELECT * FROM nope)", [(1, None, None)]),
    ("SELECT 1 FROM f WHERE $1 IN (SELECT i, tx FROM f)", [(1, None, None)]),
    ("SELECT 1 FROM f WHERE ANY(f.arr) = $1", [(1, None, None)]),
    ("SELECT 1 FROM f WHERE ANY(f.arr, 1) = $1", [(1, None, None)]),
]


@pytest.mark.parametrize(("sql", "params"), PARAMS)
def test_params(sql: str, params: list[tuple[int, str | None, bool | None]]) -> None:
    found = _core.analyze_params(sql, dialect="postgres", schema_sql=PARAMS_SCHEMA)
    assert found is not None
    assert [(index, python, nullable) for index, _, python, _, nullable, *_ in found] == params


PARAMS_ELSEWHERE = [
    ("duckdb", "SELECT a FROM g WHERE a = ? UNION TABLE g", [(1, "int", None)]),
    ("mysql", "SELECT 1 FROM f WHERE i = ANY(?)", [(1, None, None)]),
    ("mysql", "SELECT 1 FROM f WHERE ANY(?) = i", [(1, None, None)]),
    ("mysql", "SELECT 1 FROM f WHERE i = ANY(SELECT ?)", [(1, None, None)]),
    ("clickhouse", "INSERT INTO TABLE FUNCTION remote('x', f) (i) VALUES (1)", []),
    ("snowflake", "SELECT 1 FROM f WHERE tx LIKE ANY (?, ?)", [(1, "str", None), (2, "str", None)]),
    ("snowflake", "SELECT 1 FROM f WHERE tx LIKE ANY (?)", [(1, "str", None)]),
]


@pytest.mark.parametrize(("dialect", "sql", "params"), PARAMS_ELSEWHERE)
def test_params_elsewhere(
    dialect: str, sql: str, params: list[tuple[int, str | None, bool | None]]
) -> None:
    found = _core.analyze_params(sql, dialect=dialect, schema_sql=PARAMS_SCHEMA)
    assert found is not None
    assert [(index, python, nullable) for index, _, python, _, nullable, *_ in found] == params
