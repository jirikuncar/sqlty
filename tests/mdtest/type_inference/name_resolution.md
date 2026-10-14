# Name resolution

## A user table named like a system catalog

A table the schema defines always resolves, whatever its name starts with.

### postgres

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_orders (id INT, total INT);
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int | None, int | None]
```

### mysql

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_orders (id INT, total INT);
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int | None, int | None]
```

### sqlite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_orders (id INT, total INT);
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int | None, int | None]
```

## Outer columns in subqueries

A subquery reads the columns of the query around it, so they are typed. This
holds with no lint rule enabled.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE a (id INTEGER, n INTEGER, label TEXT);
CREATE TABLE b (id INTEGER, a_id INTEGER, total NUMERIC(10, 2));
```

### Unqualified

```sql
SELECT (SELECT n) FROM a
-- inferred: tuple[int | None]
```

### Aliased

```sql
SELECT (SELECT x.n + 1) FROM a x
-- inferred: tuple[int | None]
```

### Correlated

```sql
SELECT (SELECT max(b.total) FROM b WHERE b.a_id = a.id) FROM a
-- inferred: tuple[decimal.Decimal | None]
```

### The inner table shadows the outer one

```sql
SELECT (SELECT b.id FROM b WHERE b.a_id = a.id) FROM a
-- inferred: tuple[int | None]
```

### LATERAL

```sql
SELECT l.m FROM a, LATERAL (SELECT a.n * 2 AS m) l
-- inferred: tuple[int | None]
```

```sql
SELECT l.label FROM a x CROSS JOIN LATERAL (SELECT x.label) l
-- inferred: tuple[str | None]
```
