# unsupported-syntax

Default severity: **ignore**

## What it does

Reports a query that parses, but uses a construct sqlty does not type: an expression it has no rule for, `VALUES` outside PostgreSQL, `*` over a `USING` or `NATURAL` join, a column of a table function or of a `FROM`/`USING` source of a DML statement, branches of a set operation that disagree on a type. The result is not trusted, so the column stays untyped.

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

# error: [unsupported-syntax] "expression `(1, 2)` is not typed"
q = SQL[tuple[object]]("--sql\nSELECT (1, 2)")
```

## Why is this bad?

sqlty does not guess a type for a construct it does not understand. Without this rule the column is untyped and nothing says why; with it, every untyped column has a diagnostic. `NULL` and placeholders such as `$1` are untyped by nature and not reported.

## How to fix

Cast the expression to a type: `(1, 2)::TEXT`. For a `*` over a `USING` join, list the columns. To accept one case, put `# sqlty: ignore[unsupported-syntax]` on its line.

## Configuration

Off by default, because it reports code that passes today. Enable it:

```toml
[tool.sqlty.rules]
unsupported-syntax = "warn"
```

Does not need a schema, but without one only constructs are reported, not unresolved names. `error` makes `sqlty check` fail on it.
