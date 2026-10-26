# dynamic-query

Default severity: **ignore**

## What it does

Reports a call to a helper (a function with an `Annotated[..., SQL]` query parameter)
whose query is not a string literal: a variable, a constant, a call that returns the
query, or a concatenation. A psycopg composition is `unchecked-query` instead, and a
helper's own query parameter forwarded to another helper is not reported: the calls of
the outer helper are checked.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
dynamic-query = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...


def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T:
    return fetch_all(query, form)[0]


def build() -> LiteralString: ...


NAMES = "SELECT name FROM users"
# error: [dynamic-query] "query is not a string literal, so it is not checked"
names = fetch_all(NAMES, str)
# error: [dynamic-query] "query is not a string literal, so it is not checked"
rows = fetch_all(build(), int)
ids = fetch_one("SELECT id FROM users", int)
```

## Why is this bad?

sqlty reads queries from the source text, so it never sees these: the row type is not
compared with the query and the parameters are not checked. Nothing reports them by
default, so you cannot tell which calls sqlty skipped. `sqlty check --statistics`
counts them under `dynamic-query`.

## Fix

Pass the query as a string literal at the call. To audit a codebase, set the rule to
`warn` and look at what it lists.
