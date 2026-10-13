# Unparseable constraint drops the whole table

A clause the SQL parser rejects doesn't hide the table: it is retried without its table
constraints and `CHECK` / `REFERENCES` clauses, which don't affect column types.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## TRIM(BOTH FROM x) in a CHECK constraint

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    CONSTRAINT users_name_check CHECK (length(TRIM(BOTH FROM name)) > 0)
);
```

```sql
SELECT id, name FROM users
-- inferred: tuple[int, str]
```

## ON DELETE SET NULL with a column list

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, org_id INTEGER, PRIMARY KEY (org_id, id));
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    org_id INTEGER NOT NULL,
    user_id INTEGER,
    FOREIGN KEY (org_id, user_id) REFERENCES users (org_id, id) ON DELETE SET NULL (user_id)
);
```

```sql
SELECT id, user_id FROM orders
-- inferred: tuple[int, int]
```

## Column-level CHECK and REFERENCES

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    name TEXT CHECK (length(TRIM(BOTH FROM name)) > 0) NOT NULL,
    user_id INTEGER REFERENCES users (id) ON DELETE SET NULL (user_id) NOT NULL
);
```

```sql
SELECT id, name, user_id FROM orders
-- inferred: tuple[int, str, int]
```

## Semicolon inside a dollar-quoted function body

`schema.sql`:

```sql
CREATE FUNCTION touch() RETURNS trigger AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE FUNCTION tagged() RETURNS text AS $body$ SELECT 'a;b'; $body$ LANGUAGE sql;

-- a comment; with a semicolon
CREATE TABLE events (id INTEGER PRIMARY KEY, label TEXT DEFAULT 'x;y');
```

```sql
SELECT id, label FROM events
-- inferred: tuple[int, str]
```
