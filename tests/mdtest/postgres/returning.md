# PostgreSQL RETURNING Clause

```toml
[tool.sqlty]
dialect = "postgres"
```

## INSERT RETURNING single column

```sql
INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')
RETURNING id::INTEGER
-- inferred: tuple[int]
```

## INSERT RETURNING multiple columns

```sql
INSERT INTO users (name, email)
VALUES ('Alice', 'alice@example.com')
RETURNING id::INTEGER, name::TEXT, created_at::TIMESTAMP
-- inferred: tuple[int, str, datetime.datetime]
```

## INSERT with multiple VALUES rows

```sql
INSERT INTO users (name, email)
VALUES ('Alice', 'alice@example.com'), ('Bob', 'bob@example.com')
RETURNING id::INTEGER, name::TEXT
-- inferred: tuple[int, str]
```

## INSERT ON CONFLICT DO UPDATE RETURNING

```sql
INSERT INTO users (email, name)
VALUES ('alice@example.com', 'Alice')
ON CONFLICT (email) DO UPDATE SET name = EXCLUDED.name
RETURNING id::INTEGER, name::TEXT, email::TEXT
-- inferred: tuple[int, str, str]
```

## INSERT ON CONFLICT DO NOTHING RETURNING

```sql
INSERT INTO users (email, name)
VALUES ('alice@example.com', 'Alice')
ON CONFLICT (email) DO NOTHING
RETURNING id::INTEGER
-- inferred: tuple[int]
```

## INSERT ON CONFLICT with an index predicate

The conflict target can name a partial unique index: `WHERE` before `DO` is dropped.

```sql
INSERT INTO users (email, name)
VALUES ('alice@example.com', 'Alice')
ON CONFLICT (email) WHERE name <> ')DO(' DO NOTHING
RETURNING id::INTEGER
-- inferred: tuple[int]
```

## UPDATE RETURNING single column

```sql
UPDATE users SET name = 'Bob' WHERE id = 1
RETURNING id::INTEGER
-- inferred: tuple[int]
```

## UPDATE RETURNING multiple columns

```sql
UPDATE users SET name = 'Bob', email = 'bob@new.com' WHERE id = 1
RETURNING id::INTEGER, name::TEXT, email::TEXT, updated_at::TIMESTAMP
-- inferred: tuple[int, str, str, datetime.datetime]
```

## UPDATE with FROM clause RETURNING

```sql
UPDATE orders o
SET status = 'shipped'
FROM shipments s
WHERE o.id = s.order_id
RETURNING o.id::INTEGER, o.status::TEXT, s.tracking_number::TEXT
-- inferred: tuple[int, str, str]
```

## DELETE RETURNING single column

```sql
DELETE FROM users WHERE active = false
RETURNING id::INTEGER
-- inferred: tuple[int]
```

## DELETE RETURNING multiple columns

```sql
DELETE FROM users WHERE last_login < NOW() - INTERVAL '1 year'
RETURNING id::INTEGER, name::TEXT, email::TEXT, last_login::TIMESTAMP
-- inferred: tuple[int, str, str, datetime.datetime]
```

## DML without RETURNING returns None

```sql
INSERT INTO users (name) VALUES ('test')
-- inferred: None
```

## UPDATE without RETURNING returns None

```sql
UPDATE users SET name = 'test' WHERE id = 1
-- inferred: None
```

## DELETE without RETURNING returns None

```sql
DELETE FROM users WHERE id = 1
-- inferred: None
```

## DELETE RETURNING without WHERE

`RETURNING` directly after the table name is a clause, not a table alias.

```sql
DELETE FROM users RETURNING id::INTEGER
-- inferred: tuple[int]
```

## WITH ... DELETE RETURNING

```sql
WITH stale AS (SELECT id FROM sessions WHERE expired)
DELETE FROM users USING stale WHERE users.id = stale.id
RETURNING users.id::INTEGER, users.name::TEXT
-- inferred: tuple[int, str]
```

## WITH ... UPDATE RETURNING

```sql
WITH banned AS (SELECT user_id FROM reports)
UPDATE users SET active = false FROM banned WHERE users.id = banned.user_id
RETURNING id::INTEGER
-- inferred: tuple[int]
```

## WITH ... INSERT RETURNING

```sql
WITH incoming AS (SELECT 'Alice' AS name)
INSERT INTO users (name) SELECT name FROM incoming
RETURNING id::INTEGER, name::TEXT
-- inferred: tuple[int, str]
```

## INSERT ... SELECT without RETURNING returns None

The inner SELECT feeds the insert; the statement itself returns no rows.

```sql
INSERT INTO archive SELECT id::INTEGER FROM users
-- inferred: None
```

## RETURNING * expands the target table from the schema

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
DELETE FROM users WHERE id = 1 RETURNING *
-- inferred: tuple[int | None, str | None]
```

## RETURNING reads the tables of FROM and USING

`RETURNING` reads the tables of `UPDATE ... FROM` and `DELETE ... USING`, which
are inner joined to the target, and `RETURNING *` returns their columns after the
target's. An outer join inside `FROM` keeps its NULLs.

`schema.sql`:

```sql
CREATE TABLE grand (id INTEGER PRIMARY KEY, label TEXT NOT NULL);
CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT NOT NULL, grand_id INTEGER NOT NULL REFERENCES grand (id));
CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES parent (id), note TEXT);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
UPDATE child c SET note = 'x' FROM parent p WHERE p.id = c.parent_id RETURNING c.id, p.name, name
-- inferred: tuple[int, str, str]
```

```sql
UPDATE child c SET note = 'x' FROM parent p JOIN grand g ON g.id = p.grand_id WHERE p.id = c.parent_id RETURNING c.id, g.label
-- inferred: tuple[int, str]
```

```sql
UPDATE child c SET note = 'x' FROM parent p LEFT JOIN grand g ON g.label = p.name WHERE p.id = c.parent_id RETURNING g.label
-- inferred: tuple[str | None]
```

```sql
DELETE FROM child c USING parent p JOIN grand g ON g.id = p.grand_id WHERE p.id = c.parent_id RETURNING g.label
-- inferred: tuple[str]
```

```sql
UPDATE child c SET note = 'x' FROM parent p WHERE p.id = c.parent_id RETURNING *
-- columns: id: int (INT)
-- columns: parent_id: int (INT)
-- columns: note: str (TEXT)
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: grand_id: int (INT)
```

## MERGE RETURNING

`MERGE ... RETURNING` needs PostgreSQL 17 or later.

```sql
MERGE INTO users u USING staged s ON u.id = s.id
WHEN MATCHED THEN UPDATE SET name = s.name
WHEN NOT MATCHED THEN INSERT (id, name) VALUES (s.id, s.name)
RETURNING merge_action(), u.id::INTEGER, u.name::TEXT
-- inferred: None
```

## MERGE without RETURNING returns None

```sql
MERGE INTO users u USING staged s ON u.id = s.id
WHEN MATCHED THEN DELETE
-- inferred: None
```

## MERGE RETURNING * expands the target table from the schema

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
MERGE INTO users u USING staged s ON u.id = s.id
WHEN MATCHED THEN DELETE
RETURNING *
-- inferred: tuple[int | None, str | None]
```
