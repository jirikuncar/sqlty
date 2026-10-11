# select-star-no-schema

Default severity: **error**

## What it does

Flags `SELECT *` when no schema is configured.

## Why is this bad?

Without a schema the columns behind `*` are unknown, so the row type can't
be right.

## Example

```python
from sqlty import SQL

# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
```

Configure a [schema](../../../docs/guide/schema-inference.md) or list the
columns with casts.
