# Lint Rule Configuration

## Default config — star without schema is an error

```python
from sqlty import SQL
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
```

## All rules disabled — no diagnostics

```toml
[tool.sqlty.rules]
select-star = "ignore"
select-star-no-schema = "ignore"
```

```python
from sqlty import SQL
q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
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
[tool.sqlty]
schema = "schema.sql"
[tool.sqlty.rules]
select-star = "error"
```

```python
from sqlty import SQL
# error: [select-star] "SELECT * used — expand to explicit column list"
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
```

## Uninferable wrapped query is not stale

A `--sql SELECT ...` space form is a SQL comment with no row type; only
`inline-sql-comment` reports it.

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
```
