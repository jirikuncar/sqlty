# Query Parameters

Driver placeholders (`%s`, `%b`, `%t`, `%(name)s`) are values of unknown
type, so they don't stop inference; `%%` is a literal `%`. They are read alike
in every dialect.

Like psycopg, sqlty replaces them everywhere in the query, string literals
and comments included: write `'a%%'` for a literal `%`. A query that this
makes unparseable stays unwrapped, never mistyped.

## Positional

```sql
SELECT id::INTEGER, name::TEXT FROM users WHERE id = %s
-- inferred: tuple[int, str]
```

## Named

```sql
SELECT id::INTEGER FROM users WHERE id = %(id)s AND name LIKE 'a%%'
-- inferred: tuple[int]
```

## Binary and text formats

psycopg 3 also accepts `%b` and `%t`, and any name without `)`.

```sql
SELECT id::INTEGER FROM users WHERE data = %b AND name = %(user-name)t
-- inferred: tuple[int]
```

## Escaped percent

```sql
SELECT 5 %% 2, 'a%%'::TEXT
-- inferred: tuple[str, str]
```

## Cast placeholder

```sql
SELECT %s::INTEGER, %s
-- inferred: tuple[int, str]
```

## Placeholder text in strings

`%s` in a literal is replaced too, and the literal is still a literal.

```sql
SELECT $$it's %s$$::TEXT, E'it''s %s'::TEXT, '%s'::TEXT WHERE x = %s
-- inferred: tuple[str, str, str]
```

## Mixed with `$1`

psycopg rejects a query that mixes `$1` with `%s`; this only shows that the
parser accepts both.

```sql
SELECT id::INTEGER FROM users WHERE id = %s OR id = $1
-- inferred: tuple[int]
```

## Unknown format

`%d` is not a placeholder, so the query does not parse.

```sql
SELECT id::INTEGER FROM users WHERE id = %d
-- inferred: None
```

## DML with RETURNING

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
INSERT INTO users (id, name) VALUES (%s, %(name)s) RETURNING id, name
-- inferred: tuple[int, str]
```

## Stale type

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\nSELECT id FROM users WHERE name = %(name)s")
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
q = "--sql\nSELECT CAST(id AS SIGNED) FROM users WHERE id = %s"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT CAST(id AS SIGNED) FROM users WHERE id = %s")
```

## SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```sql
SELECT CAST(id AS INTEGER) FROM users WHERE id = %s
-- inferred: tuple[int]
```

## PostgreSQL question marks

`?` placeholders are read as `$1`, `$2`, ... when the query doesn't parse as
written: in PostgreSQL `?` is also a JSONB operator, which keeps its meaning.

```sql
SELECT id::INTEGER FROM users WHERE id = ? AND name = ?
-- inferred: tuple[int]
```

```sql
SELECT id::INTEGER FROM users WHERE tags ? 'admin'
-- inferred: tuple[int]
```

```sql
SELECT id::INTEGER FROM users WHERE tags ?| array['a'] AND id = ?
-- inferred: tuple[int]
```

A query with both a JSONB `?` operator and a `?` placeholder can't be read
either way, so it stays uninferred:

```sql
SELECT id::INTEGER FROM users WHERE tags ? 'admin' AND id = ?
-- inferred: None
```

### With a schema

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT id, name FROM users WHERE id = ?
-- inferred: tuple[int, str]
```
