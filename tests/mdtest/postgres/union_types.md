# UNION column types

A set operation (`UNION`, `UNION ALL`, `INTERSECT`, `EXCEPT`) resolves each output
column's type across all branches, as PostgreSQL does: `NULL`, placeholders and
columns of unknown type yield to the other branches, numbers promote to the wider
type, and types of different categories leave the column unknown. Column names
come from the leftmost branch.

`schema.sql`:

```sql
CREATE TABLE events (id INTEGER PRIMARY KEY, amount INTEGER NOT NULL);
CREATE TABLE archived_events (id BIGINT PRIMARY KEY, amount BIGINT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Integer branches promote to the wider type

```sql
SELECT amount FROM events UNION ALL SELECT amount FROM archived_events
-- columns: amount: int (BIGINT)
```

## SUM over a promoted union is numeric

```sql
SELECT sum(amount) AS total
FROM (SELECT amount FROM events UNION ALL SELECT amount FROM archived_events) AS u
-- inferred: tuple[decimal.Decimal | None]
```

## NULL and placeholder branches take the other branch's type

```sql
SELECT NULL AS amount UNION ALL SELECT amount FROM events
-- columns: amount: int | None (INT)
```

```sql
SELECT $1 AS amount UNION ALL SELECT amount FROM archived_events
-- columns: amount: int (BIGINT)
```

## A view over a union

`schema.sql`:

```sql
CREATE TABLE events (id INTEGER PRIMARY KEY, amount INTEGER NOT NULL);
CREATE TABLE archived_events (id BIGINT PRIMARY KEY, amount BIGINT NOT NULL);
CREATE VIEW all_events AS SELECT amount FROM events UNION ALL SELECT amount FROM archived_events;
```

```sql
SELECT sum(amount) AS total FROM all_events
-- inferred: tuple[decimal.Decimal | None]
```

## INTERSECT and EXCEPT

```sql
SELECT id FROM events INTERSECT SELECT id FROM archived_events
-- columns: id: int (BIGINT)
```

```sql
SELECT amount FROM events EXCEPT SELECT amount::NUMERIC FROM archived_events
-- columns: amount: decimal.Decimal (DECIMAL)
```

## Nested set operations

```sql
SELECT id AS n FROM events
UNION SELECT amount::SMALLINT FROM events
UNION ALL (SELECT amount::DOUBLE PRECISION FROM archived_events EXCEPT SELECT NULL)
-- columns: n: float (DOUBLE)
```

## Names come from the leftmost branch

```sql
SELECT id AS key, amount AS value FROM events UNION SELECT amount, id FROM archived_events
-- columns: key: int (BIGINT)
-- columns: value: int (BIGINT)
```

## Literals take the column's type

```sql
SELECT 0 AS amount UNION ALL SELECT amount FROM archived_events
-- columns: amount: int (BIGINT)
```

```sql
SELECT 1.5 AS amount UNION ALL SELECT amount FROM events
-- columns: amount: decimal.Decimal (DECIMAL)
```

## Different categories are unknown

```sql
SELECT amount FROM events UNION SELECT 'none'::text
-- columns: amount: None (None)
```

```sql
SELECT now() AS at UNION SELECT TRUE
-- columns: at: None (None)
```

## Text and timestamps take the preferred type

```sql
SELECT 'a'::VARCHAR(10) AS t UNION SELECT 'b'::VARCHAR(20)
-- columns: t: str (VARCHAR)
```

```sql
SELECT 'a'::CHAR(1) AS t UNION SELECT 'b'::TEXT
-- columns: t: str (TEXT)
```

```sql
SELECT CURRENT_DATE AS at UNION SELECT now()
-- columns: at: datetime.datetime (TIMESTAMPTZ)
```

## Recursive CTE combines both terms

The recursive term reads the non-recursive term's types.

```sql
WITH RECURSIVE t(n) AS (
    SELECT id FROM events
    UNION ALL
    SELECT n + 1::BIGINT FROM t
)
SELECT n FROM t
-- columns: n: int (BIGINT)
```

## Enums, domains and arrays keep their type

`schema.sql`:

```sql
CREATE TYPE mood AS ENUM ('happy', 'sad');
CREATE DOMAIN percent AS integer;
CREATE TABLE a (m mood, p percent, tags TEXT[], ids INTEGER[]);
CREATE TABLE b (m mood, p percent, tags TEXT[], ids BIGINT[]);
```

```sql
SELECT m, p, tags FROM a UNION SELECT m, p, tags FROM b
-- inferred: tuple[typing.Literal['happy', 'sad'] | None, int | None, list[str] | None]
```

Arrays resolve by their elements.

```sql
SELECT ids FROM a UNION SELECT ids FROM b
-- columns: ids: list[int] | None (ARRAY<BIGINT>)
```

```sql
SELECT tags FROM a UNION SELECT ids FROM b
-- columns: tags: None (None)
```
