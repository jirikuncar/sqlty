# Lint Rule Configuration

## Default config — star is warned

```sql
SELECT * FROM users
```

```python
# error: [select-star-no-schema] "SELECT * used but no schema available"
```

## All rules disabled — no diagnostics

```toml
[sqlty]
[sqlty.rules]
select-star = "ignore"
select-star-no-schema = "ignore"
```

```sql
SELECT * FROM users
```

```python
# error:
```

## Star rule set to error

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[sqlty]
schema = "schema.sql"
[sqlty.rules]
select-star = "error"
```

```sql
SELECT * FROM users
```

```python
# error: [select-star] "SELECT * used — expand to explicit column list"
```

## Uninferable wrapped query is not stale

A `--sql SELECT ...` space form is a SQL comment with no row type; only
`inline-sql-comment` reports it.

```python
q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
```

## f-string is reported as fstring-skipped, not stale-rewrite

`rewrite` never wraps f-strings, so `stale-rewrite` would be unfixable.

```python
table = "users"
q = f"--sql\nSELECT 'x' FROM {table}"
# check: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
```

## t-string is reported as tstring-skipped

```python
table = "users"
q = t"--sql\nSELECT id FROM {table}"
# check: [tstring-skipped] "t-string with --sql prefix skipped (t-strings cannot be statically rewritten)"
```

## Skipped strings follow their rule severity

```toml
[sqlty]
[sqlty.rules]
fstring-skipped = "error"
tstring-skipped = "ignore"
```

```python
table = "users"
a = f"--sql\nSELECT id FROM {table}"
b = t"--sql\nSELECT id FROM {table}"
# check: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
```
