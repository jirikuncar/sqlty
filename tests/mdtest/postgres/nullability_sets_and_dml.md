# Nullability of set operations and DML (PostgreSQL)

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
-- inferred: tuple[int, str | None]
```

```sql
DELETE FROM users WHERE age IS NOT NULL RETURNING id, age
-- inferred: tuple[int, int | None]
```

```sql
INSERT INTO users (name) VALUES ('a') RETURNING *
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: nick: str | None (TEXT)
-- columns: age: int | None (INT)
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
