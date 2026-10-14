# nullable-mismatch

Default severity: **ignore**

With `preview = true` the default is **error**.

## What it does

Reports a field of a user-defined row type (or an item of a helper's `tuple[...]` row
type) that is not optional while its query column can be NULL: the column comes from a
schema column without `NOT NULL` / `PRIMARY KEY`, from the optional side of an outer join,
or from an expression that can yield NULL.

```toml
[tool.sqlty]
schema = "schema.sql"
preview = true
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
schema, is never reported.

## Configuration

Nullability inference is a preview feature: it changes what `sqlty rewrite` writes, so
it is off until the next minor release. `preview = true` in `[tool.sqlty]` (or
`--preview` on `sqlty rewrite` and `sqlty check`) turns it on, and with it this rule as an
error. To use the rule without preview, set its severity:

```toml
[tool.sqlty.rules]
nullable-mismatch = "warn"
```

A field annotated `T | None` is accepted for a column that cannot be NULL; only
`redundant-optional` reports that.
