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
CREATE VIEW names AS SELECT full_name FROM users;
COMMENT ON VIEW names IS 'Public names';
COMMENT ON COLUMN names.full_name IS 'As shown';
CREATE TABLE gone (id INT);
COMMENT ON TABLE gone IS 'Dropped';
DROP TABLE gone;
CREATE TABLE gone (id INT);
CREATE TABLE old (id INT);
COMMENT ON COLUMN old.id IS 'Kept';
COMMENT ON TABLE old IS '';
ALTER TABLE old RENAME TO kept;
ALTER TABLE kept RENAME COLUMN id TO key;
ALTER TABLE kept ALTER COLUMN key TYPE BIGINT;
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

### Computed select item

The comment of a column a computed item reads is not the result column's.

```python
q = "--sql\nSELECT LENGTH(full_name) || full_na<CURSOR>me AS x FROM users"
# hover: Result column `x`: `str | None` (`TEXT`)
# hover:
# hover: Typed by `common.concat.text-right`: `||` with text on either side concatenates into text.
# hover:
# hover: PostgreSQL 17 documentation, 9.4, Table 9.9 SQL String Functions and Operators (anynonarray || text → text)
```

### View column

```python
q = "--sql\nSELECT 1 FROM names n WHERE n.full_na<CURSOR>me = ''"
# hover: Column `full_name` of `n`: `TEXT NULL`
# hover:
# hover: As shown
```

### Dropped table

A table created again after a `DROP` has none of the comments of the one dropped.

```python
q = "--sql\nSELECT 1 FROM go<CURSOR>ne"
# hover: Table `public.gone`
# hover: - `id` `INT NULL`
```

### Renamed table and column

An empty comment is none.

```python
q = "--sql\nSELECT 1 FROM kept WHERE ke<CURSOR>y = 1"
# hover: Column `key` of `kept`: `BIGINT NULL`
# hover:
# hover: Kept
```

```python
q = "--sql\nSELECT 1 FROM ke<CURSOR>pt"
# hover: Table `public.kept`
# hover: - `key` `BIGINT NULL`
```

## MySQL

`MODIFY` and `CHANGE` define the column again, and keep only the comment they write.

`schema.sql`:

```sql
CREATE TABLE users (id INT PRIMARY KEY COMMENT 'Key', name TEXT, email TEXT COMMENT 'Old') COMMENT = 'Before';
ALTER TABLE users COMMENT = 'People';
ALTER TABLE users MODIFY COLUMN name TEXT COMMENT 'Full name';
ALTER TABLE users MODIFY COLUMN email TEXT;
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
# hover: - `email` `TEXT NULL`
```

### Modified columns

```python
q = "--sql\nSELECT id FROM users WHERE na<CURSOR>me = ''"
# hover: Column `name` of `users`: `TEXT NULL`
# hover:
# hover: Full name
```

```python
q = "--sql\nSELECT id FROM users WHERE ema<CURSOR>il = ''"
# hover: Column `email` of `users`: `TEXT NULL`
```

## DuckDB

DuckDB writes `COMMENT ON` as PostgreSQL does.

`schema.sql`:

```sql
CREATE TABLE t (a INT);
COMMENT ON COLUMN t.a IS 'Counted';
```

```toml
[tool.sqlty]
dialect = "duckdb"
schema = "schema.sql"
```

### Column

```python
q = "--sql\nSELECT 1 FROM t WHERE <CURSOR>a = 1"
# hover: Column `a` of `t`: `INT NULL`
# hover:
# hover: Counted
```

## What a comment cannot name

A comment on an index or a constraint, on a name the schema does not define, on a column
the table does not have or on a system catalog is left out, and `IS NULL` removes one.

`schema.sql`:

```sql
CREATE TABLE t (id INT);
CREATE INDEX t_id ON t (id);
COMMENT ON INDEX t_id IS 'Index';
COMMENT ON TABLE nowhere IS 'None';
COMMENT ON COLUMN t.nope IS 'None';
COMMENT ON TABLE pg_class IS 'Catalog';
COMMENT ON COLUMN pg_class.relname IS 'Catalog';
COMMENT ON CONSTRAINT t_check ON t IS 'Unread';
COMMENT ON COLUMN t.id IS 'Kept';
CREATE VIEW v AS SELECT id FROM t;
COMMENT ON COLUMN v.id IS 'Gone';
COMMENT ON COLUMN v.id IS NULL;
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

### View column

```python
q = "--sql\nSELECT 1 FROM v WHERE i<CURSOR>d = 1"
# hover: Column `id` of `v`: `INT NULL`
```

### Column

```python
q = "--sql\nSELECT 1 FROM t WHERE i<CURSOR>d = 1"
# hover: Column `id` of `t`: `INT NULL`
# hover:
# hover: Kept
```

## MySQL comment literals

`schema.sql`:

```sql
CREATE TABLE t (id INT) COMMENT 'It''s \"t\"';
ALTER TABLE t COMMENT;
CREATE TABLE u (id INT) COMMENT 'Before';
ALTER TABLE u COMMENT = 'It\'s u';
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

### Without `=`

```python
q = "--sql\nSELECT 1 FROM <CURSOR>t"
# hover: Table `t`
# hover:
# hover: It's "t"
# hover:
# hover: - `id` `INT NULL`
```

### Escaped

```python
q = "--sql\nSELECT 1 FROM <CURSOR>u"
# hover: Table `u`
# hover:
# hover: It's u
# hover:
# hover: - `id` `INT NULL`
```
