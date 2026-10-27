# pg_catalog and information_schema

The common system relations are known without any schema file content.

```toml
[tool.sqlty]
dialect = "postgres"
```

## pg_catalog

`pg_catalog` is always searched, so its relations need no qualifier.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```sql
SELECT c.relname, c.relkind, c.reltuples, c.relhasindex FROM pg_class c
-- inferred: tuple[str, str, float, bool]
```

```sql
SELECT n.nspname, n.oid FROM pg_catalog.pg_namespace n
-- inferred: tuple[str, int]
```

```sql
SELECT a.attname, a.attnum, a.attnotnull
FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
WHERE c.relname = 'users'
-- inferred: tuple[str, int, bool]
```

```sql
SELECT typname, typtype, typlen FROM pg_type
-- inferred: tuple[str, str, int]
```

### Views

Every view of `pg_catalog` is known, generated from PostgreSQL 17 by
`scripts/refresh_pg_catalog.py`. PostgreSQL does not record whether a view column can
be NULL, so each one can.

```sql
SELECT l.pid, l.mode, l.granted, l.waitstart FROM pg_locks l
-- inferred: tuple[int | None, str | None, bool | None, datetime.datetime | None]
```

```sql
SELECT name, statement, parameter_types, prepare_time FROM pg_prepared_statements
-- inferred: tuple[str | None, str | None, list[str] | None, datetime.datetime | None]
```

```sql
SELECT relname, n_tup_ins, n_tup_upd FROM pg_stat_xact_user_tables
-- inferred: tuple[str | None, int | None, int | None]
```

### Hash functions

The hash support functions, which PostgreSQL does not document, are typed from the
catalog: they bucket or shard rows.

```sql
SELECT hashtext('a'), hashtextextended('a', 0), hashint8extended(NULL, 0)
-- inferred: tuple[int, int, int | None]
```

## information_schema

It is not on the search path: a bare `columns` is not the system view.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```sql
SELECT table_schema, table_name, table_type FROM information_schema.tables
-- inferred: tuple[str | None, str | None, str | None]
```

```sql
SELECT column_name, ordinal_position, is_nullable, data_type
FROM information_schema.columns WHERE table_name = 'users'
-- inferred: tuple[str | None, int | None, str | None, str | None]
```

```sql
SELECT column_name FROM columns
-- inferred: None
```

## Without a schema file

The built-in relations are there even when the project has no schema.

```sql
SELECT relname, relnatts FROM pg_class
-- inferred: tuple[str, int]
```

## Other dialects do not have them

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```sql
SELECT relname FROM pg_class
-- inferred: None
```

## A user table named like a system relation

As in PostgreSQL, `pg_catalog` comes first on the search path, so a bare `pg_class`
is the system one; the user's is `public.pg_class`.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_class (x INTEGER);
```

```sql
SELECT relname FROM pg_class
-- inferred: tuple[str]
```

```sql
SELECT x FROM public.pg_class
-- inferred: tuple[int | None]
```

## A table with a pg_ prefix that is not a system relation

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_orders (id INTEGER, total NUMERIC(10, 2));
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int | None, decimal.Decimal | None]
```

## The same table in SQLite and MySQL

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_orders (id INTEGER, total REAL);
CREATE TABLE pg_class (x INTEGER);
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int | None, float | None]
```

```sql
SELECT x FROM pg_class
-- inferred: tuple[int | None]
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int | None, float | None]
```
