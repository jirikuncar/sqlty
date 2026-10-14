# Column names of subscripts and field access

`schema.sql`:

```sql
CREATE TYPE address AS (street TEXT, zip INTEGER);
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    tags TEXT[] NOT NULL,
    home address
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Array subscript keeps the column name

```sql
SELECT tags[1] FROM (SELECT ARRAY[name] AS tags FROM users) AS t
-- columns: tags: str | None (TEXT)
```

## Subscripts and slices of a schema column

An element has the element type; a slice keeps the array type. A cast keeps the name.

```sql
SELECT tags[1], users.tags[2], tags[1:2], tags[1]::TEXT, (tags)[1] FROM users
-- columns: tags: str | None (TEXT)
-- columns: tags: str | None (TEXT)
-- columns: tags: list[str] (ARRAY<TEXT>)
-- columns: tags: str | None (TEXT)
-- columns: tags: str | None (TEXT)
```

## Field access of a composite

The field names the column, and a known composite types it.

```sql
SELECT (home).street, (users.home).zip FROM users
-- columns: street: str | None (TEXT)
-- columns: zip: int | None (INT)
```

## ARRAY literals resolve a common element type

As for `UNION`, PostgreSQL resolves `ARRAY[1.5, 2]` to `numeric[]`; elements of
different categories leave the array unknown.

```sql
SELECT ARRAY[1, 2], ARRAY[1.5, 2], ARRAY[name, NULL], ARRAY[1, TRUE] FROM users
-- columns: array: list[int] (ARRAY<INT>)
-- columns: array: list[decimal.Decimal] (ARRAY<DECIMAL>)
-- columns: array: list[str] (ARRAY<TEXT>)
-- columns: array: None (None)
```
