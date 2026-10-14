# ALTER TYPE and DROP in the schema

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## ALTER TYPE ... ADD VALUE

A label added after the table is declared is part of the `Literal`; `BEFORE` and
`AFTER` place it, and `IF NOT EXISTS` skips a label that is there.

`schema.sql`:

```sql
CREATE TYPE status AS ENUM ('new', 'done');
CREATE TABLE tasks (id INTEGER PRIMARY KEY, status status NOT NULL);
ALTER TYPE status ADD VALUE 'running' AFTER 'new';
ALTER TYPE status ADD VALUE IF NOT EXISTS 'done';
ALTER TYPE status ADD VALUE 'archived';
ALTER TYPE status ADD VALUE 'draft' BEFORE 'new';
```

```sql
SELECT status FROM tasks
-- inferred: tuple[typing.Literal['draft', 'new', 'running', 'done', 'archived']]
```

## ALTER TYPE ... RENAME

`schema.sql`:

```sql
CREATE TYPE status AS ENUM ('new', 'done');
ALTER TYPE status RENAME VALUE 'done' TO 'finished';
ALTER TYPE status RENAME TO task_status;
CREATE TABLE tasks (id INTEGER PRIMARY KEY, status task_status);
```

```sql
SELECT status FROM tasks
-- inferred: tuple[typing.Literal['new', 'finished']]
```

## An ALTER TYPE the catalog cannot follow

The type keeps its name, but what it holds is not trusted: the composite's fields
are unknown, and an enum is no longer a `Literal`. `OWNER TO` changes neither.

`schema.sql`:

```sql
CREATE TYPE pair AS (a INTEGER, b TEXT);
CREATE TYPE other AS (a INTEGER, b TEXT);
CREATE TYPE status AS ENUM ('new', 'done');
CREATE TYPE kind AS ENUM ('x', 'y');
CREATE TABLE t (p pair, o other, s status, k kind);
ALTER TYPE pair ADD ATTRIBUTE c BOOLEAN;
ALTER TYPE status SET SCHEMA archive;
ALTER TYPE kind OWNER TO app;
```

```sql
SELECT (p).a FROM t
-- inferred: None
```

```sql
SELECT (o).a FROM t
-- inferred: tuple[int]
```

```sql
SELECT s FROM t
-- inferred: None
```

```sql
SELECT k FROM t
-- inferred: tuple[typing.Literal['x', 'y']]
```

## DROP TABLE and DROP VIEW

`schema.sql`:

```sql
CREATE TABLE old_users (id INTEGER);
CREATE TABLE users (id INTEGER, name TEXT);
CREATE VIEW names AS SELECT name FROM users;
DROP TABLE old_users;
DROP VIEW names;
DROP TABLE IF EXISTS never_existed;
```

A dropped table is no longer in the schema, so its columns are unknown:

```sql
SELECT id FROM old_users
-- inferred: None
```

```sql
SELECT name FROM names
-- inferred: None
```

```sql
SELECT name FROM users
-- inferred: tuple[str]
```

## DROP and CREATE again

The table defined last wins, whichever order the statements come in.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER);
DROP TABLE users;
CREATE TABLE users (id TEXT, name TEXT);
```

```sql
SELECT * FROM users
-- inferred: tuple[str, str]
```

## DROP TYPE and DROP DOMAIN

`schema.sql`:

```sql
CREATE TYPE status AS ENUM ('new');
CREATE DOMAIN percent AS INTEGER;
CREATE TABLE t (s status, p percent);
DROP TYPE status;
DROP DOMAIN percent;
```

```sql
SELECT p FROM t
-- inferred: None
```

## DROP SCHEMA

`schema.sql`:

```sql
CREATE TABLE scratch.tmp (id INTEGER);
CREATE TABLE users (id INTEGER);
DROP SCHEMA scratch CASCADE;
```

```sql
SELECT id FROM scratch.tmp
-- inferred: None
```

```sql
SELECT id FROM users
-- inferred: tuple[int]
```
