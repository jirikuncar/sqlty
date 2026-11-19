# extra-param

Default severity: **warn**

## What it does

Reports keys a call passes that its query has no placeholder for: a dict
literal or `**params` with a name the query doesn't use, or any key for a query
without placeholders. It checks the calls
[`param-count-mismatch`](param-count-mismatch.md) checks.

## Why is this bad?

psycopg, sqlite3 and aiosqlite ignore such a key, so the value never reaches
the query: often a misspelled name, or a query that lost its placeholder.

## Example

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def execute(query: Annotated[LiteralString, SQL], params: dict[str, object]) -> None: ...


# error: [extra-param] "call passes extra parameter 'name'"
execute("SELECT id FROM users WHERE id = %(id)s", {"id": 1, "name": "a"})
# error: [extra-param] "call passes extra parameter 'id'"
execute("SELECT id FROM users", {"id": 1})
```

## Fix

Remove the key, or add its placeholder to the query. Make it fail the check, or
turn it off:

```toml
[tool.sqlty.rules]
extra-param = "error"
```
