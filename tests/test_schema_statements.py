"""Schema statements the catalog reads around: what a query on the schema then sees."""

import pytest

from sqlty import core

CASES = {
    "alter missing table, not reshaping": (
        "postgres",
        "ALTER TABLE nope OWNER TO x; CREATE TABLE t (a INT);",
        "SELECT * FROM t",
        "a::INT",
    ),
    "alter a view": (
        "postgres",
        "CREATE VIEW v AS SELECT 1 AS a; ALTER TABLE v OWNER TO x;",
        "SELECT * FROM v",
        "a::None",
    ),
    "foreign key to nothing": (
        "postgres",
        "CREATE TABLE t (a INT); ALTER TABLE t ADD CONSTRAINT f FOREIGN KEY (a) REFERENCES nope (id);",
        "SELECT * FROM t",
        "a::INT",
    ),
    "inherited rename": (
        "postgres",
        "CREATE TABLE p (a INT); CREATE TABLE c (b INT) INHERITS (p); CREATE TABLE d (e INT); ALTER TABLE p RENAME TO q;",
        "SELECT * FROM c",
        "a::INT, b::INT",
    ),
    "enum value missing": (
        "postgres",
        "CREATE TYPE m AS ENUM ('a'); ALTER TYPE m RENAME VALUE 'x' TO 'y'; CREATE TABLE t (a m);",
        "SELECT * FROM t",
        "a::M",
    ),
    "type renamed twice": (
        "postgres",
        "CREATE TYPE m AS ENUM ('a'); CREATE TABLE t (a m); ALTER TYPE m RENAME TO n; ALTER TYPE n RENAME TO o;",
        "SELECT * FROM t",
        "a::O",
    ),
    "type owner": (
        "postgres",
        "CREATE TYPE m AS (x INT); ALTER TYPE m OWNER TO u; CREATE TABLE t (a m);",
        "SELECT * FROM t",
        "a::M",
    ),
    "after missing column": (
        "mysql",
        "CREATE TABLE t (a INT); ALTER TABLE t ADD COLUMN c INT AFTER nope;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "untyped column": (
        "postgres",
        "CREATE TABLE t (a INT); ALTER TABLE t ADD COLUMN c;",
        "SELECT * FROM t",
        "*::None",
    ),
    "change missing column": (
        "mysql",
        "CREATE TABLE t (a INT); ALTER TABLE t CHANGE COLUMN nope c INT;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "rename to quoted": (
        "postgres",
        'CREATE TABLE t (a INT); ALTER TABLE t RENAME COLUMN a TO "B";',
        "SELECT * FROM t",
        "B::INT",
    ),
    "column options": (
        "postgres",
        "CREATE TABLE t (a INT); ALTER TABLE t ALTER COLUMN a OPTIONS (SET x 'y');",
        "SELECT * FROM t",
        "a::INT",
    ),
    "drop index": (
        "mysql",
        "CREATE TABLE t (a INT, UNIQUE KEY i (a)); ALTER TABLE t DROP INDEX i;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "no row security": (
        "postgres",
        "CREATE TABLE t (a INT); ALTER TABLE t DISABLE ROW LEVEL SECURITY;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "generated fk name": (
        "postgres",
        "CREATE TABLE p (id INT PRIMARY KEY); CREATE TABLE t (a INT REFERENCES p); ALTER TABLE t DROP CONSTRAINT t_a_fkey;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "typed table": (
        "postgres",
        "CREATE TYPE m AS (x INT); CREATE TABLE t (a INT); ALTER TABLE t OF m;",
        "SELECT * FROM t",
        "*::None",
    ),
    "identifier name": (
        "snowflake",
        "CREATE TABLE IDENTIFIER('t') (a INT); CREATE TABLE u (b INT); DROP TABLE IDENTIFIER('u');",
        "SELECT * FROM u",
        "b::INT",
    ),
    "view of a forced view": (
        "postgres",
        "CREATE VIEW v1 AS SELECT a FROM nope; CREATE VIEW v2 AS SELECT a FROM v1;",
        "SELECT * FROM v2",
        "a::None",
    ),
    "unique index on nothing": (
        "postgres",
        "CREATE UNIQUE INDEX i ON nope (a); CREATE VIEW v AS SELECT 1 AS a; CREATE UNIQUE INDEX j ON v (a); CREATE TABLE t (a INT);",
        "SELECT * FROM t",
        "a::INT",
    ),
    "trigger on nothing": (
        "postgres",
        "CREATE TRIGGER tr BEFORE INSERT ON nope FOR EACH ROW EXECUTE FUNCTION f(); CREATE TABLE t (a INT);",
        "SELECT * FROM t",
        "a::INT",
    ),
    "mysql drop trigger": (
        "mysql",
        "CREATE TABLE t (a INT); CREATE TABLE u (b INT); CREATE VIEW v AS SELECT 1 AS c; CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW SET NEW.a = 1; DROP TRIGGER tr;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "sqlite keys": (
        "sqlite",
        "CREATE TABLE t (a INTEGER, b TEXT PRIMARY KEY NOT NULL, c TEXT PRIMARY KEY, PRIMARY KEY (nope)); CREATE TABLE u (a INTEGER, PRIMARY KEY (lower(a)));",
        "SELECT * FROM t",
        "a::INT, b::TEXT, c::TEXT",
    ),
    "empty table": (
        "postgres",
        "CREATE TABLE t (); CREATE TABLE u (a INT);",
        "SELECT * FROM u",
        "a::INT",
    ),
    "table of a constant": (
        "postgres",
        "CREATE TABLE t AS SELECT CASE WHEN random() > 0 THEN 1 ELSE 2 END AS a, 'x' AS b;",
        "SELECT * FROM t",
        "a::INT, b::TEXT",
    ),
    "view with cte": (
        "postgres",
        "CREATE VIEW v AS WITH c AS (SELECT 1 AS a) SELECT a FROM c;",
        "SELECT * FROM v",
        "a::None",
    ),
    "drops": (
        "postgres",
        "CREATE TABLE t (a INT); CREATE VIEW v AS SELECT 1 AS a; DROP INDEX nope; DROP TYPE IF EXISTS nope; DROP DOMAIN IF EXISTS nope; DROP FUNCTION IF EXISTS nope; DROP SEQUENCE IF EXISTS s; DROP SCHEMA IF EXISTS x;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "drop one overload": (
        "postgres",
        "CREATE FUNCTION f(a INT) RETURNS INT AS 'x' LANGUAGE sql; CREATE FUNCTION f(a TEXT) RETURNS TEXT AS 'x' LANGUAGE sql; DROP FUNCTION f(INT);",
        "SELECT f('x') AS a",
        "a::TEXT",
    ),
    "drop a view twice": (
        "postgres",
        "CREATE VIEW v AS SELECT 1 AS a; DROP VIEW v; DROP VIEW IF EXISTS v; CREATE TABLE t (a INT);",
        "SELECT * FROM t",
        "a::INT",
    ),
    "swap": (
        "snowflake",
        "CREATE TABLE t (a INT); CREATE TABLE u (b INT); ALTER TABLE t SWAP WITH u;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "renamed parent beside a view": (
        "postgres",
        "CREATE TABLE p (a INT); CREATE TABLE c (b INT) INHERITS (p); CREATE VIEW v AS SELECT 1 AS x; ALTER TABLE p RENAME COLUMN a TO z; ALTER TABLE p RENAME TO q;",
        "SELECT * FROM c",
        "z::INT, b::INT",
    ),
    "value added to a composite": (
        "postgres",
        "CREATE TYPE m AS (x INT); ALTER TYPE m ADD VALUE 'z'; CREATE TABLE t (a m);",
        "SELECT * FROM t",
        "a::M",
    ),
    "unnamed foreign key dropped": (
        "mysql",
        "CREATE TABLE p (id INT PRIMARY KEY); CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES p (id)); ALTER TABLE t DROP FOREIGN KEY t_ibfk_1;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "identifier lookups": (
        "snowflake",
        "CREATE TABLE IDENTIFIER('t') (a INT); CREATE TABLE u (b INT); ALTER TABLE IDENTIFIER('t') ADD COLUMN c INT; CREATE UNIQUE INDEX i ON IDENTIFIER('t') (a); DROP SCHEMA IDENTIFIER('s');",
        "SELECT * FROM u",
        "b::INT",
    ),
    "catalog table dropped": (
        "postgres",
        "CREATE TABLE pg_catalog.x (a INT); CREATE VIEW v AS SELECT 1 AS a; DROP TABLE pg_catalog.x; CREATE TABLE t (a INT);",
        "SELECT * FROM t",
        "a::INT",
    ),
    "unreadable alters": (
        "postgres",
        "ALTER TYPE nope ADD ATTRIBUTE x INT; ALTER FUNCTION nope(int) RENAME TO g; CREATE FUNCTION; CREATE TABLE t (a INT);",
        "SELECT * FROM t",
        "a::INT",
    ),
    "untyped columns": ("postgres", "CREATE TABLE t (a, b INT);", "SELECT * FROM t", "*::None"),
    "column added to the parent only, then renamed": (
        "postgres",
        "CREATE TABLE p (a INT); CREATE TABLE c (b INT) INHERITS (p); CREATE TABLE p2 (x INT); CREATE TABLE c2 (y INT) INHERITS (p2); ALTER TABLE ONLY p ADD COLUMN d INT; ALTER TABLE p RENAME COLUMN d TO e; ALTER TABLE p RENAME TO q;",
        "SELECT * FROM c",
        "a::INT, b::INT",
    ),
    "two types renamed": (
        "postgres",
        "CREATE TYPE m AS ENUM ('a'); CREATE TYPE k AS ENUM ('b'); CREATE TABLE t (a m, b k); ALTER TYPE m RENAME TO n; ALTER TYPE k RENAME TO j;",
        "SELECT * FROM t",
        "a::N, b::J",
    ),
    "function renamed before it exists": (
        "postgres",
        "ALTER FUNCTION nope RENAME TO g; CREATE TABLE t (a INT);",
        "SELECT * FROM t",
        "a::INT",
    ),
    "missing column altered": (
        "postgres",
        "CREATE TABLE t (a INT); ALTER TABLE t ALTER COLUMN nope SET NOT NULL;",
        "SELECT * FROM t",
        "a::INT",
    ),
    "partitioned table with an unreadable constraint": (
        "postgres",
        "CREATE TABLE t (a INT, CONSTRAINT c bogus stuff) PARTITION BY RANGE (a) bogus",
        "SELECT * FROM t",
        "a::INT",
    ),
    "like with an unreadable constraint": (
        "postgres",
        "CREATE TABLE p (a INT); CREATE TABLE t (LIKE p, b INT, CONSTRAINT c bogus stuff)",
        "SELECT * FROM t",
        "a::INT, b::INT",
    ),
    "range type": (
        "postgres",
        "CREATE TYPE r AS RANGE (SUBTYPE = int4); CREATE TABLE t (a r)",
        "SELECT * FROM t",
        "a::R",
    ),
    "unreadable drop of a sequence": (
        "postgres",
        "CREATE TABLE t (a INT); DROP SEQUENCE s bogus",
        "SELECT * FROM t",
        "a::INT",
    ),
    "unreadable rename": (
        "mysql",
        "CREATE TABLE a (x INT); RENAME TABLE a TO b bogus",
        "SELECT * FROM a",
        "x::INT",
    ),
    "renames in turn": (
        "mysql",
        "CREATE TABLE a (x INT); CREATE TABLE c (y INT); RENAME TABLE a TO b, c TO d",
        "SELECT * FROM b, d",
        "x::INT, y::INT",
    ),
    "table as query if not exists": (
        "postgres",
        "CREATE TABLE IF NOT EXISTS t AS SELECT 1 AS a",
        "SELECT * FROM t",
        "a::None",
    ),
}

SKIPPED = {
    "table without columns": ("mysql", "CREATE TABLE t", []),
    "unreadable view": ("mysql", "CREATE VIEW v AS bogus", [("view", "v")]),
    "unreadable domain": ("postgres", "CREATE DOMAIN d AS bogus (((", []),
    "unreadable index": ("postgres", "CREATE INDEX bogus (((", []),
    "unreadable drop of a table": (
        "postgres",
        "CREATE TABLE t (a INT); DROP TABLE t bogus",
        [("table", "t")],
    ),
    "unreadable drop of a type": (
        "postgres",
        "CREATE TYPE e AS ENUM ('a'); DROP TYPE e bogus",
        [("type", "e")],
    ),
    "renamed view": (
        "postgres",
        "CREATE VIEW v AS SELECT 1 AS a; ALTER VIEW v RENAME TO w",
        [("table", "v")],
    ),
    "virtual table outside SQLite": ("postgres", "CREATE VIRTUAL TABLE v USING fts5(a)", []),
    "R*Tree without an id": ("sqlite", "CREATE VIRTUAL TABLE r USING rtree(+)", [("table", "r")]),
}


@pytest.mark.parametrize(("dialect", "schema", "expected"), SKIPPED.values(), ids=SKIPPED.keys())
def test_skipped(dialect: str, schema: str, expected: list[tuple[str, str]]) -> None:
    assert core.schema_skipped_objects(schema, dialect=dialect) == expected


@pytest.mark.parametrize(
    ("dialect", "schema", "query", "expected"), CASES.values(), ids=CASES.keys()
)
def test_columns(dialect: str, schema: str, query: str, expected: str) -> None:
    columns = core.analyze_select(query, dialect=dialect, schema_sql=schema)
    assert columns is not None
    assert ", ".join(f"{c.name}::{c.sql_type}" for c in columns) == expected
