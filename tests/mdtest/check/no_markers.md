# Files without markers produce no diagnostics

`sqlty check` skips the Rust checker for a file that holds no `--sql` / `-- sql`
marker, `SQL[` wrapper call or `UNTYPED`. The result is the same as running it.

```python
x = "SELECT id FROM users"
# error:
```

## A marker anywhere still runs the checker

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
x = "--sql\nSELECT 1::INTEGER AS n"
```

## `SQL` named without a subscript

A module that names `SQL` but never subscripts it is not read for row types.

```python
SQL = "SELECT id FROM users"
query = SQL.lower()
# error:
```

## Wrapper calls spelled apart

The wrapper and its `[` may be apart by blanks or a line continuation, and are still read.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
```

```python
from typing import TypedDict

class Row(TypedDict):
    id: str

# error: [column-type-mismatch] "Row.id is str but query column 'id' infers int"
a = SQL [Row]("--sql\nSELECT id FROM users")
# error: [column-type-mismatch] "Row.id is str but query column 'id' infers int"
b = SQL\
[Row]("--sql\nSELECT id FROM users")
```
