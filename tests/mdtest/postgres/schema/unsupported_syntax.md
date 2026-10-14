# What the catalog cannot follow

Syntax sqlparser cannot read must not produce a wrong type. A table changed by a
statement the catalog cannot follow has an unknown shape: its columns get no type
and a `*` over it is left alone.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## An unreadable ALTER TABLE

`schema.sql`:

```sql
CREATE TABLE moved (id INTEGER);
CREATE TABLE kept (id INTEGER);
ALTER TABLE moved SET SCHEMA archive;
ALTER TABLE kept OWNER TO app;
```

```sql
SELECT id FROM moved
-- inferred: None
```

```sql
SELECT id FROM kept
-- inferred: tuple[int | None]
```

## An ALTER of a column that does not exist

PostgreSQL rejects it; the catalog skips the statement.

`schema.sql`:

```sql
CREATE TABLE t (a INTEGER);
ALTER TABLE t ALTER COLUMN missing TYPE TEXT;
```

```sql
SELECT a FROM t
-- inferred: tuple[int | None]
```

## An unreadable CREATE TABLE is skipped

`schema.sql`:

```sql
CREATE TABLE t (a INTEGER, b ??? );
```

```sql
SELECT a FROM t
-- inferred: None
```

## CREATE TABLE AS

A table built from a query is not in the catalog.

`schema.sql`:

```sql
CREATE TABLE base (a INTEGER);
CREATE TABLE copy AS SELECT a FROM base;
```

```sql
SELECT a FROM copy
-- inferred: None
```

## Other dialects keep reading CREATE statements only

`ALTER TABLE` and `DROP` are PostgreSQL semantics.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INT, name VARCHAR(20));
CREATE TABLE `shop`.`orders` (id INT, total DECIMAL(10, 2));
ALTER TABLE users ADD COLUMN age INT;
DROP TABLE orders;
```

```sql
SELECT * FROM users
-- inferred: tuple[int | None, str | None]
```

```sql
SELECT total FROM shop.orders
-- inferred: tuple[decimal.Decimal | None]
```
