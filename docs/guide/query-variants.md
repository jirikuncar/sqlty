# Query variants

A query built from names bound in branches is checked once per path that builds it.

```toml
[tool.sqlty]
schema = "schema.sql"
preview = true
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN NOT NULL);
```

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...


def users(named: bool) -> list[tuple[int, str]]:
    if named:
        where = "name IS NOT NULL"
    else:
        where = "TRUE"
    # error: [nullable-mismatch] "(variant: line 13)"
    return fetch_all("SELECT id, name FROM users WHERE " + where, tuple[int, str])
```

Without `preview`, the query is not a string literal, so `check` skips it as
[`dynamic-query`](../reference/rules.md#dynamic-query), which is off by default. With it,
`check` reads both queries and checks each against `tuple[int, str]`. The one ending in
`WHERE TRUE` returns users without a name, which `str` does not admit.

!!! note "A preview feature"
    Query variants are off unless `preview = true` is set in `[tool.sqlty]`. They may
    change before they are on by default.

## What is followed

`check` walks the function around the call, without running it. It forks at `if`,
`elif`, `else`, `match` and conditional expressions (`a if flag else b`), and drops the
paths that `return` or `raise` before the call. Every part of the query must be static
on every path:

| Part | Example |
|------|---------|
| A string literal | `"SELECT id FROM users"` |
| `+` and `+=` of static parts | `query += " AND active"` |
| A name whose every binding is static | `where = "TRUE"` in one branch, `where = "active"` in the other |
| A `Literal` parameter | `def find(column: Literal["id", "name"])` |
| A lookup in a dict of literals | `ORDER[direction]` with `ORDER = {"asc": "ASC", "desc": "DESC"}` |
| The return of a function of the module built the same way | `"SELECT id FROM users WHERE " + where(active)` |
| A `psycopg.sql` composition of these | `sql.SQL("... {}").format(sql.SQL(where))` |

A condition tested twice takes the same branch both times, as long as nothing it reads
is assigned in between. Names bound together in one branch stay together. A `case` of
string literals keeps only the values it matches, so `case "foo": return` drops `foo`
from the variants after the `match`. A function called without an argument reads the
default of its parameter.

## What is not followed

A part that is not static leaves the query unchecked, all of its variants included: one
unchecked path could hide any query. The call is then `dynamic-query`, or
`unchecked-query` for a `psycopg.sql` composition. These are not static:

- an f-string, or a value from a call to anything but a function of the module;
- a name bound in a loop, a `try` or a `with` block, or by a walrus;
- a parameter whose type is not a `Literal` of strings, unless the argument is static.

```toml
[tool.sqlty]
schema = "schema.sql"
preview = true

[tool.sqlty.rules]
dynamic-query = "warn"
```

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...


def users(columns: list[LiteralString]) -> list[tuple[int]]:
    query: LiteralString = "SELECT id FROM users WHERE TRUE"
    for column in columns:
        query += " AND " + column
    # error: [dynamic-query] "query is not a string literal, so it is not checked"
    return fetch_all(query, tuple[int])
```

## Reporting

A diagnostic every variant has is reported once, as for a single query. Any other names
the lines of the bindings that select the first variant having it, such as
`(variant: line 13)`: the line of `where = "TRUE"` above.

A call with more variants than `max-query-variants` (default `64`) is reported with
their count, `query has 128 variants, more than max-query-variants (64)`, rather than
checked on some of them.

```toml
[tool.sqlty]
preview = true
max-query-variants = 128
```

`sqlty check --statistics` counts each variant as a query.
