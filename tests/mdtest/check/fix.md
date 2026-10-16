# Fixes

`sqlty check --fix` applies `core.fix_source()`: it wraps unwrapped queries,
expands `SELECT *` from a schema, and refreshes stale `tuple[...]` and
`UNTYPED` row types. Rules set to `"ignore"` are not fixed.

## Unwrapped query is rewritten

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Stale tuple type is refreshed

```python
from sqlty import SQL

q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Refreshed type adds its import

```python
from sqlty import SQL

q = SQL[tuple[str]]("--sql\nSELECT created_at::TIMESTAMP, token::UUID FROM events")
# fixed: import datetime
# fixed: import uuid
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[datetime.datetime, uuid.UUID]]("--sql\nSELECT created_at::TIMESTAMP, token::UUID FROM events")
```

## Existing datetime usage adds no import

Only types written by the fix bring imports.

```python
from datetime import datetime

from sqlty import SQL

now = datetime.now()
q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: from datetime import datetime
# fixed:
# fixed: from sqlty import SQL
# fixed:
# fixed: now = datetime.now()
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## User types are left alone

```python
from typing import TypedDict

from sqlty import SQL


class User(TypedDict):
    id: str


q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# fixed: from typing import TypedDict
# fixed:
# fixed: from sqlty import SQL
# fixed:
# fixed:
# fixed: class User(TypedDict):
# fixed:     id: str
# fixed:
# fixed:
# fixed: q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
```

## Uninferable query keeps its type

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT x, y FROM pairs")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT x, y FROM pairs")
```

## Formatter-split wrapper keeps its layout

```python
from sqlty import SQL

q = SQL[tuple[str]](
    "--sql\nSELECT id::INTEGER FROM users"
)
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]](
# fixed:     "--sql\nSELECT id::INTEGER FROM users"
# fixed: )
```

## UNTYPED becomes the inferred type

```python
from sqlty import SQL
from sqlty import UNTYPED

known = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: known = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## UNTYPED import stays while still used

```python
from sqlty import SQL
from sqlty import UNTYPED

known = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
unknown = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
# fixed: from sqlty import SQL
# fixed: from sqlty import UNTYPED
# fixed:
# fixed: known = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: unknown = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

## Module import style

```toml
[tool.sqlty]
import-style = "import sqlty as sy"
```

```python
import sqlty as sy

a = sy.SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
b = sy.SQL[sy.UNTYPED]("--sql\nSELECT name::TEXT FROM users")
# fixed: import sqlty as sy
# fixed:
# fixed: a = sy.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: b = sy.SQL[tuple[str]]("--sql\nSELECT name::TEXT FROM users")
```

## Ignored rules are not fixed

```toml
[tool.sqlty.rules]
stale-type = "ignore"
stale-rewrite = "ignore"
```

```python
from sqlty import SQL

q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
r = "--sql\nSELECT id::INTEGER FROM users"
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: r = "--sql\nSELECT id::INTEGER FROM users"
```

## Star expansion

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

### Single-line string

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int | None, str | None]]("--sql\nSELECT id::INT, name::TEXT FROM users")
```

### Triple-quoted string

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("""--sql
SELECT *
FROM users
WHERE id = 1
""")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str | None]]("""--sql
# fixed: SELECT id::INT, name::TEXT FROM users
# fixed: WHERE id = 1
# fixed: """)
```

### Unwrapped star query is wrapped and expanded

```python
q = "--sql\nSELECT * FROM users"
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int | None, str | None]]("--sql\nSELECT id::INT, name::TEXT FROM users")
```

### Escapes in the query are left alone

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users WHERE name = 'a\tb'")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int | None, str]]("--sql\nSELECT * FROM users WHERE name = 'a\tb'")
```

### Star rule ignored

```toml
[tool.sqlty.rules]
select-star = "ignore"
```

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int | None, str | None]]("--sql\nSELECT * FROM users")
```

## Formatter trailing comma is kept

```python
from sqlty import SQL

q = SQL[tuple[str]](
    "--sql\nSELECT id::INTEGER FROM users",
)
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]](
# fixed:     "--sql\nSELECT id::INTEGER FROM users",
# fixed: )
```

## UNTYPED import stays while used elsewhere

```python
from sqlty import SQL
from sqlty import UNTYPED

q = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
missing = UNTYPED
# fixed: from sqlty import SQL
# fixed: from sqlty import UNTYPED
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: missing = UNTYPED
```

## Space after the marker becomes a newline

```python
q = "--sql SELECT id::INTEGER FROM users"
r = r'--sql SELECT name::TEXT FROM users'
s = f"--sql SELECT {column} FROM users"
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: r = r'--sql SELECT name::TEXT FROM users'
# fixed: s = f"--sql SELECT {column} FROM users"
```

## Space after the marker in a non-SQL string is kept

Only a string that `inline-sql-comment` reports is changed: the word after the
marker must start a statement (`SELECT` with a column, `DELETE FROM`,
`UPDATE t SET`, ...).

```python
msg = "--sql flag enables it"
todo = "--sql delete me later"
bare = "--sql select"
note = "--sql UPDATE stuff"
# fixed: msg = "--sql flag enables it"
# fixed: todo = "--sql delete me later"
# fixed: bare = "--sql select"
# fixed: note = "--sql UPDATE stuff"
```

## Space after the marker in a `u` string becomes a newline

A `u` prefix does not change a string, so it is fixed and wrapped like a plain one.

```python
q = u"--sql SELECT id::INTEGER FROM users"
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int]](u"--sql\nSELECT id::INTEGER FROM users")
```

## Apostrophes in comments and escaped quotes open no string

```python
# it's a helper
a = 'it\'s'
q = "--sql SELECT id::INTEGER FROM users"
b = br'\'"'
r = "--sql\nSELECT name::TEXT FROM users"
# that's all
# fixed: from sqlty import SQL
# fixed: # it's a helper
# fixed: a = 'it\'s'
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: b = br'\'"'
# fixed: r = SQL[tuple[str]]("--sql\nSELECT name::TEXT FROM users")
# fixed: # that's all
```

## Star without a schema keeps its type

The row type of `*` is unknown without a schema, so it is not refreshed.

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT * FROM users")
r = SQL[tuple[int, int]]("--sql\nSELECT id, * FROM users")
s = SQL[tuple[int]]("--sql\nSELECT u.* FROM users u")
t = SQL[tuple[int]]("--sql\nselect  * from users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT * FROM users")
# fixed: r = SQL[tuple[int, int]]("--sql\nSELECT id, * FROM users")
# fixed: s = SQL[tuple[int]]("--sql\nSELECT u.* FROM users u")
# fixed: t = SQL[tuple[int]]("--sql\nselect  * from users")
```

## Star the schema cannot resolve keeps its type

A subquery or CTE has no schema entry, and `USING` merges the joined `id`
columns, so these annotations are kept.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE TABLE orders (id INTEGER, user_id INTEGER);
```

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT * FROM (SELECT id FROM users) s")
r = SQL[tuple[int]]("--sql\nWITH c AS (SELECT id FROM users) SELECT * FROM c")
s = SQL[tuple[int, str, int]]("--sql\nSELECT * FROM users JOIN orders USING (id)")
t = SQL[tuple[int, int, str]]("--sql\nSELECT id, * FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int | None]]("--sql\nSELECT * FROM (SELECT id FROM users) s")
# fixed: r = SQL[tuple[int | None]]("--sql\nWITH c AS (SELECT id FROM users) SELECT * FROM c")
# fixed: s = SQL[tuple[int, str, int]]("--sql\nSELECT * FROM users JOIN orders USING (id)")
# fixed: t = SQL[tuple[int | None, int | None, str | None]]("--sql\nSELECT id, id::INT, name::TEXT FROM users")
```

## Stars over joins keep their qualifiers

`u.*` expands to `u.`-qualified columns, so it stays unambiguous next to the
joined table. A bare `*` over a join lists every table's columns, each
qualified by its alias or name.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE TABLE orders (id INTEGER, user_id INTEGER);
```

```python
from sqlty import SQL

q = SQL[tuple[int, str, int]]("--sql\nSELECT u.*, o.id FROM users u JOIN orders o ON o.user_id = u.id")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str | None, int | None]]("--sql\nSELECT u.id::INT, u.name::TEXT, o.id FROM users u JOIN orders o ON o.user_id = u.id")
```

```python
from sqlty import SQL

q = SQL[tuple[int, str, int, int]]("--sql\nSELECT * FROM users u JOIN orders o ON o.user_id = u.id")
r = SQL[tuple[int, str, int, int]]("--sql\nSELECT * FROM users, orders")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str | None, int | None, int]]("--sql\nSELECT u.id::INT, u.name::TEXT, o.id::INT, o.user_id::INT FROM users u JOIN orders o ON o.user_id = u.id")
# fixed: r = SQL[tuple[int | None, str | None, int | None, int | None]]("--sql\nSELECT users.id::INT, users.name::TEXT, orders.id::INT, orders.user_id::INT FROM users, orders")
```

## Plain string after an f-string is wrapped

A prefixed literal is skipped as a whole, so the next plain string is paired
and wrapped correctly.

```python
table = "users"
f = f"--sql\nSELECT id FROM {table}"
q = "--sql\nSELECT id::INTEGER FROM users"
# fixed: from sqlty import SQL
# fixed: table = "users"
# fixed: f = f"--sql\nSELECT id FROM {table}"
# fixed: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Triple-quoted wrappers are fixed

```python
from sqlty import SQL

a = SQL[tuple[str]]("""--sql
SELECT 1 AS n""")
b = SQL[tuple[str]]("""-- sql\nSELECT 1 AS n""")
# fixed: from sqlty import SQL
# fixed:
# fixed: a = SQL[tuple[int]]("""--sql
# fixed: SELECT 1 AS n""")
# fixed: b = SQL[tuple[int]]("""-- sql\nSELECT 1 AS n""")
```

## Escaped CRLF marker is fixed

```python
from sqlty import SQL

a = "--sql\r\nSELECT 1 AS n"
b = SQL[tuple[str]]("--sql\r\nSELECT 1 AS n")
# fixed: from sqlty import SQL
# fixed:
# fixed: a = SQL[tuple[int]]("--sql\r\nSELECT 1 AS n")
# fixed: b = SQL[tuple[int]]("--sql\r\nSELECT 1 AS n")
```
