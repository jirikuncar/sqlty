# ALTER TABLE in the schema

The schema is a catalog, not a list of `CREATE TABLE` statements: `ALTER TABLE`
changes the columns a table has, so a migration history works as a schema file.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## ADD COLUMN

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
ALTER TABLE users ADD COLUMN age INTEGER NOT NULL;
ALTER TABLE users ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ, ADD COLUMN IF NOT EXISTS name TEXT;
```

```sql
SELECT * FROM users
-- inferred: tuple[int, str, int, datetime.datetime]
```

## DROP COLUMN

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, legacy TEXT);
ALTER TABLE users DROP COLUMN legacy;
ALTER TABLE users DROP COLUMN IF EXISTS never_existed;
```

```sql
SELECT * FROM users
-- inferred: tuple[int, str]
```

## RENAME COLUMN

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, full_name TEXT);
ALTER TABLE users RENAME COLUMN full_name TO name;
```

```sql
SELECT id, name FROM users
-- inferred: tuple[int, str]
```

## ALTER COLUMN TYPE

`schema.sql`:

```sql
CREATE TABLE events (id INTEGER, payload TEXT, at TIMESTAMP);
ALTER TABLE events ALTER COLUMN id TYPE BIGINT, ALTER COLUMN at SET DATA TYPE TIMESTAMPTZ;
ALTER TABLE events ALTER COLUMN payload TYPE JSONB USING payload::jsonb;
```

```sql
SELECT * FROM events
-- inferred: tuple[int, typing.Any, datetime.datetime]
```

## SET and DROP NOT NULL, defaults

Nullability is kept in the catalog. It does not change an inferred type yet, and
`SET DEFAULT` changes nothing.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT NOT NULL, age INTEGER);
ALTER TABLE users ALTER COLUMN id SET NOT NULL;
ALTER TABLE users ALTER COLUMN name DROP NOT NULL;
ALTER TABLE users ALTER COLUMN age SET DEFAULT 0;
```

```sql
SELECT * FROM users
-- inferred: tuple[int, str, int]
```

## RENAME TO

`schema.sql`:

```sql
CREATE TABLE accounts (id INTEGER, balance NUMERIC(10, 2));
ALTER TABLE accounts RENAME TO wallets;
```

```sql
SELECT id, balance FROM wallets
-- inferred: tuple[int, decimal.Decimal]
```

## ONLY, IF EXISTS and statements without a table

`ALTER TABLE IF EXISTS` on a table the schema does not define is skipped.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER);
ALTER TABLE IF EXISTS ONLY nothing ADD COLUMN x INTEGER;
ALTER TABLE ONLY users ADD COLUMN y TEXT;
```

```sql
SELECT * FROM users
-- inferred: tuple[int, str]
```

## Constraints and ownership change no column

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, email TEXT);
ALTER TABLE users ADD CONSTRAINT users_pk PRIMARY KEY (id);
ALTER TABLE users ADD CONSTRAINT users_email UNIQUE (email);
ALTER TABLE users DROP CONSTRAINT users_email;
ALTER TABLE users OWNER TO app;
ALTER TABLE users ENABLE ROW LEVEL SECURITY;
```

```sql
SELECT * FROM users
-- inferred: tuple[int, str]
```

## Used in a view

A view reads the table as it was when the view was created.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE VIEW names AS SELECT * FROM users;
ALTER TABLE users ADD COLUMN age INTEGER;
```

```sql
SELECT * FROM names
-- inferred: tuple[int, str]
```
