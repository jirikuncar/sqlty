# Operator and predicate result types

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW(),
    big BIGINT,
    ratio DOUBLE PRECISION
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Predicates are bool, concatenation is text

```sql
SELECT
    email IS NULL AS no_email,
    id = 1 AS is_first,
    NOT (id > 1) AS small,
    name || '!' AS shout
FROM users
-- inferred: tuple[bool, bool, bool, str]
```

## Other predicates

```sql
SELECT
    id IN (1, 2) AS a,
    id BETWEEN 1 AND 5 AS b,
    name LIKE 'a%' AS c,
    name ILIKE 'a%' AS d,
    email IS DISTINCT FROM name AS e,
    id > 1 AND id < 5 AS f,
    EXISTS (SELECT 1 FROM users) AS g
FROM users
-- columns: a: bool (BOOLEAN)
-- columns: b: bool (BOOLEAN)
-- columns: c: bool (BOOLEAN)
-- columns: d: bool (BOOLEAN)
-- columns: e: bool (BOOLEAN)
-- columns: f: bool (BOOLEAN)
-- columns: g: bool (BOOLEAN)
```

## JSON operators

```sql
SELECT
    '{}'::jsonb ->> 'a' AS a,
    '{}'::jsonb -> 'a' AS b,
    '{}'::jsonb #>> '{a}' AS c,
    '{}'::jsonb #> '{a}' AS d,
    '{}'::jsonb @> '{}' AS e,
    '{}'::jsonb ? 'a' AS f
-- columns: a: str (TEXT)
-- columns: b: dict (JSONB)
-- columns: c: str (TEXT)
-- columns: d: dict (JSONB)
-- columns: e: bool (BOOLEAN)
-- columns: f: bool (BOOLEAN)
```

## Arithmetic promotes numeric operands

```sql
SELECT id + 1 AS a, id + big AS b, id * ratio AS c, big - id AS d, id + %(n)s AS e FROM users
-- columns: a: int (INT)
-- columns: b: int (BIGINT)
-- columns: c: float (DOUBLE)
-- columns: d: int (BIGINT)
-- columns: e: int (INT)
```

## Date arithmetic

```sql
SELECT created_at + INTERVAL '1 day' AS a, created_at - created_at AS b FROM users
-- columns: a: datetime.datetime (TIMESTAMP)
-- columns: b: datetime.timedelta (INTERVAL)
```
