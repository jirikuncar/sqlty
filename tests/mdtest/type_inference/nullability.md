# Nullability inference

A column that can be NULL infers `T | None`: a schema column without `NOT NULL` or
`PRIMARY KEY`, a column of the optional side of an outer join, an expression that can yield
NULL. Nullability is tracked next to the type, and only what the schema or the query proves
is optional: a column of unknown nullability keeps its plain type.

## Schema columns without NOT NULL

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL,
    name TEXT,
    age INTEGER
);
```

```sql
SELECT id, email, name, age FROM users
-- inferred: tuple[int, str, str | None, int | None]
```

`SELECT *` expands to the same columns:

```sql
SELECT * FROM users
-- columns: id: int (INT)
-- columns: email: str (TEXT)
-- columns: name: str | None (TEXT)
-- columns: age: int | None (INT)
```

## Table-level and ALTER TABLE constraints

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE a (x INTEGER, y INTEGER, z INTEGER, PRIMARY KEY (x));
ALTER TABLE a ALTER COLUMN y SET NOT NULL;
```

```sql
SELECT x, y, z FROM a
-- inferred: tuple[int, int, int | None]
```

## Casts without a schema stay non-optional

Nothing says `id` can be NULL, so a cast is not made optional.

```toml
[tool.sqlty]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
-- inferred: tuple[int, str]
```

## A cast keeps the nullability of its operand

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, age INTEGER);
```

```sql
SELECT id::BIGINT, age::BIGINT, (age) FROM users
-- inferred: tuple[int, int | None, int | None]
```

## Outer joins null-extend the optional side

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
CREATE TABLE tags (post_id INTEGER NOT NULL, tag TEXT NOT NULL);
```

A `LEFT JOIN` makes every column of the right relation optional, even `NOT NULL` ones.

```sql
SELECT u.id, u.name, p.id, p.title FROM users u LEFT JOIN posts p ON p.user_id = u.id
-- inferred: tuple[int, str, int | None, str | None]
```

`RIGHT JOIN` null-extends the left side, `FULL JOIN` both.

```sql
SELECT u.id, p.id FROM users u RIGHT JOIN posts p ON p.user_id = u.id
-- inferred: tuple[int | None, int]
```

```sql
SELECT u.id, p.id FROM users u FULL JOIN posts p ON p.user_id = u.id
-- inferred: tuple[int | None, int | None]
```

An inner join and a cross join null-extend nothing.

```sql
SELECT u.id, p.id FROM users u JOIN posts p ON p.user_id = u.id
-- inferred: tuple[int, int]
```

## Nested outer joins

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL);
CREATE TABLE tags (post_id INTEGER NOT NULL, tag TEXT NOT NULL);
```

Each outer join extends the relation it adds; the earlier ones stay as they were.

```sql
SELECT u.id, p.id, t.tag
FROM users u
LEFT JOIN posts p ON p.user_id = u.id
LEFT JOIN tags t ON t.post_id = p.id
-- inferred: tuple[int, int | None, str | None]
```

An outer join to a parenthesised group null-extends all of the group.

```sql
SELECT u.id, p.id, t.tag
FROM users u
LEFT JOIN (posts p JOIN tags t ON t.post_id = p.id) ON p.user_id = u.id
-- inferred: tuple[int, int | None, str | None]
```

An inner join after a `RIGHT JOIN` keeps the null-extension of the left side.

```sql
SELECT u.id, p.id, t.tag
FROM users u
RIGHT JOIN posts p ON p.user_id = u.id
JOIN tags t ON t.post_id = p.id
-- inferred: tuple[int | None, int, str]
```

## Derived tables, CTEs and views carry their nullability

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, nick TEXT);
CREATE VIEW nicknames AS SELECT id, nick FROM users;
CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL);
```

```sql
SELECT id, nick FROM nicknames
-- inferred: tuple[int, str | None]
```

```sql
WITH u AS (SELECT id, nick FROM users) SELECT id, nick FROM u
-- inferred: tuple[int, str | None]
```

```sql
SELECT d.id, d.nick FROM (SELECT id, nick FROM users) d
-- inferred: tuple[int, str | None]
```

A derived table on the optional side of an outer join is optional as a whole.

```sql
SELECT p.id, d.id FROM posts p LEFT JOIN (SELECT id FROM users) d ON d.id = p.user_id
-- inferred: tuple[int, int | None]
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INT NOT NULL AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50));
CREATE TABLE posts (id INT NOT NULL, user_id INT NOT NULL);
```

```sql
SELECT u.id, u.name, p.id FROM users u LEFT JOIN posts p ON p.user_id = u.id
-- inferred: tuple[int, str | None, int | None]
```

## SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE posts (id INTEGER NOT NULL, user_id INTEGER NOT NULL);
```

```sql
SELECT u.id, u.name, p.id FROM users u LEFT JOIN posts p ON p.user_id = u.id
-- inferred: tuple[int, str | None, int | None]
```

## Rewrite emits T | None

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from sqlty import SQL

q = "--sql\nSELECT id, name FROM users"
# rewritten: q = SQL[tuple[int, str | None]]("--sql\nSELECT id, name FROM users")
```

The row type is stale once nullability is on: `sqlty check` flags the old annotation.

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int, str]] should be SQL[tuple[int, str | None]]"
q = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users")
```
