# COALESCE ignores NULL and placeholder arguments

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Placeholder first

```sql
SELECT coalesce(%(since)s, created_at) AS since FROM users
-- inferred: tuple[datetime.datetime | None]
```

## NULL first

```sql
SELECT coalesce(NULL, created_at) AS created_at FROM users
-- inferred: tuple[datetime.datetime | None]
```

## GREATEST and LEAST skip them too

```sql
SELECT greatest(%(a)s, NULL, id) AS g, least(NULL, name) AS l FROM users
-- columns: g: int (INT)
-- columns: l: str (TEXT)
```

## Nothing known stays unknown

```sql
SELECT coalesce(NULL, %(x)s) AS x
-- columns: x: None (None)
```

## A constant next to a value of unknown type

A parameter or an argument of unknown type can hold any value of the common
type, so the constants' `Literal` widens to it. `NULL` adds no value.

```sql
SELECT coalesce($1, 1) AS a, coalesce(nope(), 'n') AS b, coalesce(NULL, 1) AS c
-- columns: a: int (None)
-- columns: b: str (None)
-- columns: c: typing.Literal[1] (None)
```
