# Views in the schema

A `CREATE VIEW` in the schema becomes a table whose columns are typed from its
query. Views are analyzed in order, so a view may read the views before it.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## View columns

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
CREATE VIEW user_names AS SELECT id, name FROM users;
```

```sql
SELECT id, name FROM user_names
-- inferred: tuple[int, str]
```

`SELECT *` and qualified columns work too:

```sql
SELECT * FROM user_names
-- inferred: tuple[int, str]
```

```sql
SELECT v.name FROM user_names v
-- inferred: tuple[str]
```

## Column list, aliases and expressions

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, total NUMERIC(10, 2));
CREATE VIEW order_stats (uid, orders, spent) AS
    SELECT user_id, COUNT(*), SUM(total) FROM orders GROUP BY user_id;
CREATE VIEW labelled AS SELECT id AS order_id, total::TEXT AS label FROM orders;
```

```sql
SELECT uid, orders, spent FROM order_stats
-- inferred: tuple[int | None, int, decimal.Decimal | None]
```

```sql
SELECT order_id, label FROM labelled
-- inferred: tuple[int, str | None]
```

## Views over views

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
CREATE VIEW adults AS SELECT id, name, age FROM users WHERE age >= 18;
CREATE VIEW adult_names AS SELECT name FROM adults;
```

```sql
SELECT name FROM adult_names
-- inferred: tuple[str]
```

## CREATE OR REPLACE, materialized views and qualified names

The last definition wins, and `public.v` matches `v`.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
CREATE VIEW v AS SELECT name FROM users;
CREATE OR REPLACE VIEW v AS SELECT id, name FROM users;
CREATE MATERIALIZED VIEW public.counts AS SELECT age, COUNT(*) AS n FROM users GROUP BY age;
```

```sql
SELECT id, name FROM public.v
-- inferred: tuple[int, str]
```

```sql
SELECT age, n FROM counts
-- inferred: tuple[int | None, int]
```

## pg_dump style views

Parenthesised joins and qualified names, as `pg_dump` writes them.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, total NUMERIC(10, 2));
CREATE VIEW public.user_orders AS
 SELECT u.name,
    o.total
   FROM (public.users u
     JOIN public.orders o ON ((o.user_id = u.id)))
  WHERE (o.total > (0)::numeric);
```

```sql
SELECT name, total FROM user_orders
-- inferred: tuple[str, decimal.Decimal]
```

## Unresolvable columns stay unknown

A column the schema cannot type is unknown, as it is in a CTE; the rest of the
view still resolves.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE VIEW mixed AS SELECT id, name, id + 1 AS next_id, missing_column FROM users;
CREATE VIEW opaque AS SELECT * FROM not_in_schema;
```

```sql
SELECT id, name, next_id, missing_column FROM mixed
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: next_id: int (INT)
-- columns: missing_column: None (None)
```

```sql
SELECT * FROM opaque
-- columns: *: None (None)
```

## CREATE TABLE ... AS

A table created from a query has the query's column names and types. It has no
constraints, so every column can be NULL and a value a `CHECK` limited may now be
any text. An `ALTER TABLE` that changes its columns makes them unknown.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL CHECK (name <> ''), role TEXT NOT NULL CHECK (role IN ('admin', 'member')));
CREATE TABLE users_backup AS SELECT id, name, role, 'v1' AS version FROM users;
CREATE TABLE IF NOT EXISTS user_ids AS SELECT id FROM users WITH NO DATA;
CREATE TABLE altered AS SELECT id FROM users;
ALTER TABLE altered ADD COLUMN note TEXT;
```

```sql
SELECT * FROM users_backup
-- columns: id: int | None (INT)
-- columns: name: str | None (TEXT)
-- columns: role: str | None (TEXT)
-- columns: version: str | None (TEXT)
```

```sql
SELECT * FROM user_ids
-- inferred: tuple[int | None]
```

```sql
SELECT id, note FROM altered
-- inferred: None
```
