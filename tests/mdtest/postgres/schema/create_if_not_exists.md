# IF NOT EXISTS and OR REPLACE

As in PostgreSQL, `CREATE ... IF NOT EXISTS` keeps what is already there, and
`CREATE OR REPLACE` replaces it.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## CREATE TABLE IF NOT EXISTS

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER);
CREATE TABLE IF NOT EXISTS users (id TEXT, name TEXT);
CREATE TABLE IF NOT EXISTS orders (id INTEGER);
```

```sql
SELECT * FROM users
-- inferred: tuple[int | None]
```

```sql
SELECT * FROM orders
-- inferred: tuple[int | None]
```

## CREATE VIEW IF NOT EXISTS and OR REPLACE

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE VIEW ids AS SELECT id FROM users;
CREATE VIEW IF NOT EXISTS ids AS SELECT id, name FROM users;
CREATE VIEW names AS SELECT id FROM users;
CREATE OR REPLACE VIEW names AS SELECT id, name FROM users;
```

```sql
SELECT * FROM ids
-- inferred: tuple[int | None]
```

```sql
SELECT * FROM names
-- inferred: tuple[int | None, str | None]
```

## CREATE OR REPLACE FUNCTION

`schema.sql`:

```sql
CREATE FUNCTION f() RETURNS integer LANGUAGE sql AS 'SELECT 1';
CREATE OR REPLACE FUNCTION f() RETURNS text LANGUAGE sql AS 'SELECT ''a''';
```

```sql
SELECT f()
-- inferred: tuple[str]
```
