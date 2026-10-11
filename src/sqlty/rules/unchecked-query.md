# unchecked-query

Default severity: **warn**

## What it does

Flags `--sql` queries that are not plain string literals: a raw or bytes
string, alone or implicitly concatenated with others, or a `SQL[T](...)`
argument that holds anything besides string literals.

## Why is this bad?

sqlty reads queries from the source text, so it neither rewrites nor checks
these: the row type is never inferred or compared.

## Example

```python
from sqlty import SQL

# error: [unchecked-query] "--sql query is not plain string literals, so it is not checked or rewritten"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM " + table)
# error: [unchecked-query] "--sql query is not plain string literals, so it is not checked or rewritten"
r = r"--sql\nSELECT id::INTEGER FROM users WHERE name ~ '\d'"
```

## Fix

Write the query as one plain literal, or as implicitly concatenated plain
literals, and pass values as query parameters.
