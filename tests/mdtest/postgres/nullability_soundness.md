# Nullability: no false non-null (PostgreSQL)

Where NULL can occur, the column is optional. These are the cases where a looser
reading would call a nullable value non-null.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE t (
    id INTEGER PRIMARY KEY,
    a INTEGER,
    g INTEGER NOT NULL,
    tags TEXT[] NOT NULL,
    doc JSONB NOT NULL
);
```

## CASE: a WHEN that does not match also covers NULL

`a > 5` is NULL for a NULL `a`, so the `ELSE` branch can still see a NULL.

```sql
SELECT CASE WHEN a > 5 THEN 1 ELSE a END FROM t
-- inferred: tuple[int | None]
```

A condition that cannot be NULL does narrow the later branches.

```sql
SELECT CASE WHEN a IS NULL THEN 0 ELSE a END FROM t
-- inferred: tuple[int]
```

The right side of an `OR` is read where the left one is false or NULL.

```sql
SELECT a > 5 OR a = 1, (a > 5 OR a IS NOT NULL) FROM t
-- inferred: tuple[bool | None, bool | None]
```
