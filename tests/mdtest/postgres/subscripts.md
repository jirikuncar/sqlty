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
-- columns: tags: str (TEXT)
```

## Subscripts and slices of a schema column

An element has the element type; a slice keeps the array type. A cast keeps the name.

```sql
SELECT tags[1], users.tags[2], tags[1:2], tags[1]::TEXT, (tags)[1] FROM users
-- columns: tags: str (TEXT)
-- columns: tags: str (TEXT)
-- columns: tags: list[str] (ARRAY<TEXT>)
-- columns: tags: str (TEXT)
-- columns: tags: str (TEXT)
```

## Field access of a composite

The field names the column, and a known composite types it.

```sql
SELECT (home).street, (users.home).zip FROM users
-- columns: street: str (TEXT)
-- columns: zip: int (INT)
```

## ARRAY literals of mixed element types stay unknown

PostgreSQL resolves `ARRAY[1.5, 2]` to `numeric[]`; only elements of one type are typed.

```sql
SELECT ARRAY[1, 2], ARRAY[1.5, 2], ARRAY[name, NULL] FROM users
-- columns: array: list[int] (ARRAY<INT>)
-- columns: array: None (None)
-- columns: array: list[str] (ARRAY<TEXT>)
```
