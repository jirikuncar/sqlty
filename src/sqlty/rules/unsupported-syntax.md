# unsupported-syntax

Default severity: **warn**

## What it does

Reports a query that parses, but uses a construct sqlty does not type: an expression it has no rule for, `VALUES` outside PostgreSQL (unless it is the source of an `INSERT` without `RETURNING`, where nothing reads its types), `*` over a `USING` or `NATURAL` join, a column of a table function or of a `FROM`/`USING` source of a DML statement, branches of a set operation that disagree on a type. The result is not trusted, so the column stays untyped.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unsupported-syntax = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [unsupported-syntax] "expression `1 OPERATOR(pg_catalog.+) 1` is not typed"
q = SQL[tuple[object]]("--sql\nSELECT 1 OPERATOR(pg_catalog.+) 1")
```

### `INSERT ... VALUES` outside PostgreSQL

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.rules]
stale-rewrite = "ignore"
```

```python
# error:
a = "--sql\nINSERT INTO users (id, name) VALUES (1, 'a')"
# error: [unsupported-syntax] "a VALUES list is not typed"
b = "--sql\nINSERT INTO users (id, name) VALUES (1, 'a') RETURNING id"
# error: [unsupported-syntax] "a VALUES list is not typed"
c = "--sql\nSELECT * FROM (VALUES (1, 'a')) AS v"
```

## Why is this bad?

sqlty does not guess a type for a construct it does not understand. Without this rule the column is untyped and nothing says why; with it, every untyped column has a diagnostic. `NULL` and placeholders such as `$1` are untyped by nature and not reported.

## How to fix

Cast the expression to a type: `(1 OPERATOR(pg_catalog.+) 1)::INTEGER`. For a `*` over a `USING` join, list the columns. To accept one case, put `# sqlty: ignore[unsupported-syntax]` on its line.

## Configuration

A warning by default: the SQL is valid, but sqlty cannot type the construct, so the query is
not fully checked. A warning tells you why without failing CI on sqlty's gaps. Make it
fail, or turn it off:

```toml
[tool.sqlty.rules]
unsupported-syntax = "error"
```

Does not need a schema, but without one only constructs are reported, not unresolved names.
