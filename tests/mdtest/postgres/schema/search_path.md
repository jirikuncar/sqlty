# search_path and schema-qualified names

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Qualified names pick their schema

Two tables with the same name in different schemas stay apart.

`schema.sql`:

```sql
CREATE TABLE billing.accounts (id INTEGER, balance NUMERIC(10, 2));
CREATE TABLE crm.accounts (id INTEGER, name TEXT);
```

```sql
SELECT id, balance FROM billing.accounts
-- inferred: tuple[int | None, decimal.Decimal | None]
```

```sql
SELECT id, name FROM crm.accounts
-- inferred: tuple[int | None, str | None]
```

Without a `search_path` entry that decides, a bare name with two candidates is
unknown: sqlty does not guess.

```sql
SELECT id FROM accounts
-- inferred: None
```

## SET search_path

A bare name is looked up in the `search_path` in order. Statements after a `SET
search_path` create their tables in its first schema.

`schema.sql`:

```sql
CREATE TABLE public.accounts (id INTEGER);
SET search_path TO billing, public;
CREATE TABLE accounts (id INTEGER, balance NUMERIC(10, 2));
CREATE TABLE crm.accounts (id INTEGER, name TEXT);
```

```sql
SELECT * FROM accounts
-- inferred: tuple[int | None, decimal.Decimal | None]
```

```sql
SELECT * FROM public.accounts
-- inferred: tuple[int | None]
```

## The search_path at the end of the file decides bare names

`schema.sql`:

```sql
SET search_path = crm;
CREATE TABLE accounts (id INTEGER, name TEXT);
SET search_path = billing;
CREATE TABLE accounts (id INTEGER, balance NUMERIC(10, 2));
```

```sql
SELECT * FROM accounts
-- inferred: tuple[int | None, decimal.Decimal | None]
```

```sql
SELECT * FROM crm.accounts
-- inferred: tuple[int | None, str | None]
```

## pg_dump style set_config

`schema.sql`:

```sql
SELECT pg_catalog.set_config('search_path', '', false);
CREATE TABLE public.users (id INTEGER, name TEXT);
```

```sql
SELECT * FROM users
-- inferred: tuple[int | None, str | None]
```

## A schema with one definition still needs the search_path

A bare name resolves only through the `search_path`. A table that lives in a single
other schema is not found without it (see `name_resolution.md` for the diagnostics).

`schema.sql`:

```sql
CREATE TABLE app.users (id INTEGER, name TEXT);
CREATE TYPE app.mood AS ENUM ('ok', 'bad');
CREATE TABLE app.checkins (id INTEGER, mood app.mood);
```

```sql
SELECT * FROM users
-- inferred: None
```

```sql
SELECT * FROM app.users
-- inferred: tuple[int | None, str | None]
```

```sql
SELECT mood FROM app.checkins
-- inferred: tuple[typing.Literal['ok', 'bad'] | None]
```

## Qualified types and views

`schema.sql`:

```sql
CREATE TYPE a.kind AS ENUM ('x', 'y');
CREATE TYPE b.kind AS ENUM ('p', 'q');
CREATE TABLE t (ka a.kind, kb b.kind);
CREATE VIEW a.v AS SELECT ka FROM t;
```

```sql
SELECT ka, kb FROM t
-- inferred: tuple[typing.Literal['x', 'y'] | None, typing.Literal['p', 'q'] | None]
```

```sql
SELECT ka FROM a.v
-- inferred: tuple[typing.Literal['x', 'y'] | None]
```

## Table names in other schemas do not match

A qualified name looks in its own schema only.

`schema.sql`:

```sql
CREATE TABLE a.users (id INTEGER);
```

```sql
SELECT id FROM b.users
-- inferred: None
```

## A view reads its tables under the search_path it was created in

### A view written before its table

A view written before the table it reads waits for it, and is read under the
`search_path` of its own statement, not under a later one.

`schema.sql`:

```sql
CREATE VIEW waiting AS SELECT * FROM accounts;
CREATE TABLE accounts (id INTEGER);
CREATE SCHEMA crm;
CREATE TABLE crm.accounts (name TEXT);
SET search_path TO crm;
```

```sql
SELECT * FROM public.waiting
-- inferred: tuple[int | None]
```

### A view right after SET search_path

A view right after a `SET search_path` reads the tables of the new path.

`schema.sql`:

```sql
CREATE TABLE accounts (id INTEGER);
CREATE TABLE crm.accounts (name TEXT);
CREATE VIEW before AS SELECT * FROM accounts;
SET search_path TO crm;
CREATE VIEW after AS SELECT * FROM accounts;
```

```sql
SELECT * FROM public.before
-- inferred: tuple[int | None]
```

```sql
SELECT * FROM crm.after
-- inferred: tuple[str | None]
```
