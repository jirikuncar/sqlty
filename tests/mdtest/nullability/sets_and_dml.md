# Nullability: set operations and DML (PostgreSQL)

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    nick TEXT,
    age INTEGER
);
CREATE TABLE admins (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    nick TEXT,
    age INTEGER NOT NULL
);
```

## UNION is nullable if either side is

```sql
SELECT id, name, nick, age FROM users UNION SELECT id, name, nick, age FROM admins
-- inferred: tuple[int, str, str | None, int | None]
```

```sql
SELECT age FROM admins UNION ALL SELECT age FROM admins
-- inferred: tuple[int]
```

```sql
SELECT age FROM admins UNION ALL SELECT age FROM users UNION ALL SELECT age FROM admins
-- inferred: tuple[int | None]
```

A literal `NULL` branch makes the column optional.

```sql
SELECT age FROM admins UNION ALL SELECT NULL::INTEGER
-- inferred: tuple[int | None]
```

## INTERSECT is nullable only if both sides are

```sql
SELECT age FROM users INTERSECT SELECT age FROM admins
-- inferred: tuple[int]
```

```sql
SELECT age FROM users INTERSECT SELECT age FROM users
-- inferred: tuple[int | None]
```

## EXCEPT follows its left side

```sql
SELECT age FROM admins EXCEPT SELECT age FROM users
-- inferred: tuple[int]
```

```sql
SELECT age FROM users EXCEPT SELECT age FROM admins
-- inferred: tuple[int | None]
```

## A set operation in a derived table

```sql
SELECT d.age FROM (SELECT age FROM admins UNION SELECT age FROM users) d
-- inferred: tuple[int | None]
```

## RETURNING reads the schema

```sql
INSERT INTO users (name) VALUES ('a') RETURNING id, name, nick, age
-- inferred: tuple[int, str, str | None, int | None]
```

```sql
UPDATE users SET nick = 'x' WHERE id = 1 RETURNING id, nick
-- inferred: tuple[int, str]
```

```sql
DELETE FROM users WHERE id = 1 RETURNING id, age
-- inferred: tuple[int, int | None]
```

```sql
INSERT INTO users (name) VALUES ('a') RETURNING *
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: nick: str | None (TEXT)
-- columns: age: int | None (INT)
```

## RETURNING a written value

A column the statement writes with a value that is never NULL is not NULL in the
rows it returns. A placeholder can be bound to `None`, so a column written from one
stays nullable.

```sql
INSERT INTO users (id, name, nick) VALUES (1, 'a', 'b'), (2, 'c', upper('d')) RETURNING nick, age
-- inferred: tuple[str, int | None]
```

```sql
INSERT INTO users (id, name, nick) VALUES (1, 'a', 'b'), (2, 'c', NULL) RETURNING nick
-- inferred: tuple[str | None]
```

```sql
INSERT INTO users (id, name, nick) VALUES (1, 'a', %(nick)s) RETURNING nick
-- inferred: tuple[str | None]
```

```sql
INSERT INTO users (id, name, nick) SELECT id, name, name FROM admins RETURNING nick
-- inferred: tuple[str]
```

```sql
UPDATE users SET nick = name || '!', age = NULL WHERE id = 1 RETURNING nick, age
-- inferred: tuple[str, int | None]
```

A row that conflicts is returned as `DO UPDATE` leaves it: a column keeps its old
value unless `SET` writes it, from `EXCLUDED` or otherwise.

```sql
INSERT INTO users (id, name, nick, age) VALUES (1, 'a', 'b', 2)
ON CONFLICT (id) DO UPDATE SET nick = EXCLUDED.nick
RETURNING nick, age
-- inferred: tuple[str, int | None]
```

```sql
INSERT INTO users (id, name, nick) VALUES (1, 'a', 'b') ON CONFLICT DO NOTHING RETURNING nick
-- inferred: tuple[str]
```

## RETURNING a column's DEFAULT

A column an `INSERT` leaves out gets its `DEFAULT`, so it is not NULL in the rows
returned when the default never is. `VALUES` shorter than the table leaves out its
last columns, and `DEFAULT VALUES` all of them.

`schema.sql`:

```sql
CREATE TABLE posts (
    id SERIAL PRIMARY KEY,
    title TEXT,
    status TEXT DEFAULT 'draft',
    created_at TIMESTAMPTZ DEFAULT now(),
    edited_at TIMESTAMPTZ DEFAULT NULL,
    score INTEGER
);
ALTER TABLE posts ALTER COLUMN score SET DEFAULT 0;
ALTER TABLE posts ALTER COLUMN status DROP DEFAULT;
```

```sql
INSERT INTO posts (title) VALUES (%(title)s) RETURNING id, title, status, created_at, edited_at, score
-- columns: id: int (INT)
-- columns: title: str | None (TEXT)
-- columns: status: str | None (TEXT)
-- columns: created_at: datetime.datetime (TIMESTAMPTZ)
-- columns: edited_at: datetime.datetime | None (TIMESTAMPTZ)
-- columns: score: int (INT)
```

```sql
INSERT INTO posts VALUES (1, 'a') RETURNING created_at, score
-- inferred: tuple[datetime.datetime, int]
```

```sql
INSERT INTO posts (title, score) VALUES ('a', NULL) RETURNING created_at, score
-- inferred: tuple[datetime.datetime, int | None]
```

```sql
INSERT INTO posts DEFAULT VALUES RETURNING created_at
-- inferred: tuple[datetime.datetime]
```

## RETURNING after WHERE

`DELETE` returns the rows its `WHERE` let through. `UPDATE` returns them as `SET`
leaves them: a column it sets is what it is set to, whatever `WHERE` proved.

```sql
DELETE FROM users WHERE nick = %(nick)s AND age IS DISTINCT FROM NULL RETURNING nick, age
-- inferred: tuple[str, int]
```

```sql
UPDATE users SET name = nick WHERE nick IS NOT NULL RETURNING name, nick
-- inferred: tuple[str, str]
```

```sql
UPDATE users SET nick = NULL WHERE nick IS NOT NULL RETURNING nick
-- inferred: tuple[str | None]
```

## ON CONFLICT

`EXCLUDED` and the target table are the same row type.

```sql
INSERT INTO users (id, name) VALUES (1, 'a')
ON CONFLICT (id) DO UPDATE SET nick = EXCLUDED.nick
RETURNING id, nick, coalesce(nick, name)
-- inferred: tuple[int, str | None, str]
```

```sql
INSERT INTO users (id, name) VALUES (1, 'a') ON CONFLICT DO NOTHING RETURNING age
-- inferred: tuple[int | None]
```

## RETURNING with a BEFORE row trigger

`RETURNING` returns the row after `BEFORE ... FOR EACH ROW` triggers have run, and
such a trigger can set any column to NULL. A column it may change stays nullable
unless the table declares it `NOT NULL`, which PostgreSQL checks after the trigger.

`schema.sql`:

```sql
CREATE TABLE progress (id INTEGER PRIMARY KEY, name TEXT NOT NULL, task TEXT);
CREATE TABLE edits (id INTEGER PRIMARY KEY, task TEXT);
CREATE TABLE logged (id INTEGER PRIMARY KEY, task TEXT);
CREATE TABLE dropped (id INTEGER PRIMARY KEY, task TEXT);
CREATE TRIGGER clear BEFORE INSERT OR UPDATE ON progress FOR EACH ROW EXECUTE FUNCTION clear_task();
CREATE TRIGGER clear_on_edit BEFORE UPDATE OF task ON edits FOR EACH ROW EXECUTE FUNCTION clear_task();
CREATE TRIGGER log AFTER INSERT ON logged FOR EACH ROW EXECUTE FUNCTION log_task();
CREATE TRIGGER audit BEFORE INSERT ON logged FOR EACH STATEMENT EXECUTE FUNCTION audit();
CREATE TRIGGER clear BEFORE INSERT ON dropped FOR EACH ROW EXECUTE FUNCTION clear_task();
DROP TRIGGER clear ON dropped;
```

```sql
INSERT INTO progress (id, name, task) VALUES (1, 'a', 'b') RETURNING name, task
-- inferred: tuple[str, str | None]
```

```sql
UPDATE progress SET task = 'a' WHERE id = 1 RETURNING task
-- inferred: tuple[str | None]
```

```sql
INSERT INTO progress (id, name, task) VALUES (1, 'a', 'b') ON CONFLICT (id) DO UPDATE SET task = EXCLUDED.task RETURNING task
-- inferred: tuple[str | None]
```

A `BEFORE UPDATE` trigger fires for `UPDATE` and for the `DO UPDATE` branch of an
upsert, not for a plain `INSERT`.

```sql
INSERT INTO edits (id, task) VALUES (1, 'a') RETURNING task
-- inferred: tuple[str]
```

```sql
UPDATE edits SET task = 'a' WHERE id = 1 RETURNING task
-- inferred: tuple[str | None]
```

```sql
INSERT INTO edits (id, task) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET task = EXCLUDED.task RETURNING task
-- inferred: tuple[str | None]
```

`AFTER` and statement-level triggers cannot change the returned row, and a dropped
trigger no longer fires.

```sql
INSERT INTO logged (id, task) VALUES (1, 'a') RETURNING task
-- inferred: tuple[str]
```

```sql
INSERT INTO dropped (id, task) VALUES (1, 'a') RETURNING task
-- inferred: tuple[str]
```
