# PostgreSQL type spellings

```toml
[tool.sqlty]
dialect = "postgres"
```

## FLOAT(p)

`FLOAT(p)` is `real` up to 24 bits of precision and `double precision` above; a
bare `FLOAT` is `double precision`.

```sql
SELECT CAST(x AS FLOAT(1)) AS a, CAST(x AS FLOAT(24)) AS b, CAST(x AS FLOAT(25)) AS c, CAST(x AS FLOAT(53)) AS d, CAST(x AS FLOAT) AS e
-- columns: a: float (FLOAT)
-- columns: b: float (FLOAT)
-- columns: c: float (DOUBLE)
-- columns: d: float (DOUBLE)
-- columns: e: float (DOUBLE)
```

## Types of other dialects

The `postgres` dialect reads type names of other dialects as their PostgreSQL
counterparts, as the default dialect for any project; sqlty does not check that
PostgreSQL has them. PostgreSQL itself rejects `BLOB`, `DATETIME` or `TINYINT`.

```sql
SELECT CAST(x AS BLOB) AS a, CAST(x AS DATETIME) AS b, CAST(x AS NVARCHAR(5)) AS c, CAST(x AS TINYINT) AS d, CAST(x AS LONGTEXT) AS e
-- columns: a: bytes (VARBINARY)
-- columns: b: datetime.datetime (DATETIME)
-- columns: c: str (VARCHAR(5))
-- columns: d: int (TINYINT)
-- columns: e: str (TEXT)
```

## Session functions without parentheses

PostgreSQL reads these reserved words as functions without parentheses. `CURRENT_SCHEMA`
is NULL when the search path names no existing schema, and `SYSTEM_USER` when the
user did not authenticate, as under `trust`:

```sql
SELECT CURRENT_USER, CURRENT_ROLE, SESSION_USER, CURRENT_SCHEMA, CURRENT_CATALOG, SYSTEM_USER
-- columns: current_user: str (NAME)
-- columns: current_role: str (NAME)
-- columns: session_user: str (NAME)
-- columns: current_schema: str | None (NAME)
-- columns: current_catalog: str (NAME)
-- columns: system_user: str | None (TEXT)
-- rules: current_user: postgres.system.name / postgres.system.name
-- rules: current_role: postgres.system.name / postgres.system.name
-- rules: session_user: postgres.system.name / postgres.system.name
-- rules: current_schema: postgres.system.current-schema / postgres.system.current-schema
-- rules: current_catalog: postgres.system.name / postgres.system.name
-- rules: system_user: postgres.system.text-nullable / postgres.system.text-nullable
```

## Geometric types

psycopg has no loader for `point`, `line`, `lseg`, `box`, `path`, `polygon` and
`circle`, so it returns their text.

```sql
SELECT CAST(x AS POINT) AS a, CAST(x AS BOX) AS b, CAST(x AS POLYGON) AS c, CAST(x AS CIRCLE) AS d
-- columns: a: str (POINT)
-- columns: b: str (BOX)
-- columns: c: str (POLYGON)
-- columns: d: str (CIRCLE)
```
