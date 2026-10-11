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

q = SQL[tuple[int]]("--sql\nVALUES (1, 2)")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nVALUES (1, 2)")
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
unknown = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
# fixed: from sqlty import SQL
# fixed: from sqlty import UNTYPED
# fixed:
# fixed: known = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: unknown = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
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
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id::INT, name::TEXT FROM users")
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
# fixed: q = SQL[tuple[int, str]]("""--sql
# fixed: SELECT id::INT, name::TEXT FROM users
# fixed: WHERE id = 1
# fixed: """)
```

### Unwrapped star query is wrapped and expanded

```python
q = "--sql\nSELECT * FROM users"
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id::INT, name::TEXT FROM users")
```

### Escapes in the query are left alone

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users WHERE name = 'a\tb'")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users WHERE name = 'a\tb'")
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
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
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
