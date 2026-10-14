# nullable-mismatch

Default severity: **error**

## What it does

Reports a field of a user-defined row type (or an item of a helper's `tuple[...]` row
type) that is not optional while its query column can be NULL: the column comes from a
schema column without `NOT NULL` / `PRIMARY KEY`, from the optional side of an outer join,
or from an expression that can yield NULL.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import TypedDict

from sqlty import SQL


class User(TypedDict):
    id: int
    name: str


# error: [nullable-mismatch] "User.name is str but query column 'name' can be NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Why is this bad?

The annotation promises a value the database does not always return. A `None` reaches
code that type checkers believe handles only `str`.

## How to fix

Make the field optional (`name: str | None`), or filter the NULLs out in the query
(`WHERE name IS NOT NULL`), or give the column a default (`COALESCE(name, '')`).

```python
from typing import TypedDict

from sqlty import SQL


class User(TypedDict):
    id: int
    name: str | None


q = SQL[User]("--sql\nSELECT id, name FROM users")
# error:
```

Fields typed `Any` or `object`, and fields whose annotation sqlty cannot resolve, are
not reported. A column whose nullability is unknown, such as `id::INTEGER` with no
schema, is never reported: sqlty only reports what the schema or the query proves.

## Configuration

A field annotated `T | None` is accepted for a column that cannot be NULL; only
[`redundant-optional`](redundant-optional.md) reports that. To lower the severity of this
rule while you add the missing `| None`, or to turn it off:

```toml
[tool.sqlty.rules]
nullable-mismatch = "warn"
```
