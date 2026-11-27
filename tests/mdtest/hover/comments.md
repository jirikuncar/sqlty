# Comments

What the schema's `COMMENT` says of a table, view or column shows under the hover's first
line, and in the documentation of its completion item. A later comment replaces it,
`IS NULL` removes it, and it follows renames.

## PostgreSQL

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT);
COMMENT ON TABLE users IS 'People who sign in';
COMMENT ON COLUMN users.name IS 'Shown on the profile';
COMMENT ON COLUMN users.email IS 'Login';
COMMENT ON COLUMN users.email IS NULL;
ALTER TABLE users RENAME COLUMN name TO full_name;
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

### Column

```python
q = "--sql\nSELECT id FROM users WHERE full_na<CURSOR>me = ''"
# hover: Column `full_name` of `users`: `TEXT NULL`
# hover:
# hover: Shown on the profile
```

### Result column

```python
q = "--sql\nSELECT full_na<CURSOR>me FROM users"
# hover: Result column `full_name`: `str | None` (`TEXT`)
# hover:
# hover: Shown on the profile
```

### Table

```python
q = "--sql\nSELECT 1 FROM use<CURSOR>rs"
# hover: Table `public.users`
# hover:
# hover: People who sign in
# hover:
# hover: - `id` `INT NOT NULL`
# hover: - `full_name` `TEXT NULL`
# hover: - `email` `TEXT NULL`
```

### Removed comment

```python
q = "--sql\nSELECT id FROM users WHERE ema<CURSOR>il = ''"
# hover: Column `email` of `users`: `TEXT NULL`
```

## MySQL

`schema.sql`:

```sql
CREATE TABLE users (id INT PRIMARY KEY COMMENT 'Key', name TEXT) COMMENT = 'People';
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

### Column

```python
q = "--sql\nSELECT name FROM users WHERE i<CURSOR>d = 1"
# hover: Column `id` of `users`: `INT NOT NULL`
# hover:
# hover: Key
```

### Table

```python
q = "--sql\nSELECT 1 FROM use<CURSOR>rs"
# hover: Table `users`
# hover:
# hover: People
# hover:
# hover: - `id` `INT NOT NULL`
# hover: - `name` `TEXT NULL`
```
