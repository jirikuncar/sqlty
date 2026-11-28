---
order: 225
group: "names"
severity: "error"
fixable: false
summary: "Call of a built-in function with arguments none of its overloads takes"
origin: "SQL"
---

# argument-count-mismatch

Default severity: **error**

## What it does

Reports a call of a built-in function that passes a number of arguments none of its overloads
takes, or `*` to a function other than `count`. The overloads come from the dialect's function
catalog, which signature help shows; optional and repeated parameters count as the documentation
writes them.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [argument-count-mismatch] "`lower` takes 1 argument, got 2: lower(text) -> text"
q = SQL[tuple[str | None]]("--sql\nSELECT lower(name, 1) FROM users")
```

## Why is this bad?

The database rejects the query when it runs: PostgreSQL with `function lower(text, integer) does
not exist`, MySQL with `Incorrect parameter count in the call to native function`.

## How to fix

Pass the arguments one of the listed overloads takes.

## Configuration

Not checked for a function the schema defines with `CREATE FUNCTION`, which may take other
arguments or have defaults, for a call with named arguments, or in a dialect without a function
catalog (SQLite).
