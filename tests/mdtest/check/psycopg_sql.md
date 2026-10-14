# psycopg sql.SQL and composed queries

Requires the optional `psycopg` dependency (`sqlty[psycopg]`).

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    total NUMERIC(10, 2) NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT NOW()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Literal sql.SQL

```python
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
names = fetch_all(sql.SQL("SELECT id FROM users"), str)
```

## format() with identifiers, joins and placeholders

```python
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

query = sql.SQL("SELECT {cols} FROM {table} WHERE id = {id}").format(
    cols=sql.SQL(", ").join([sql.Identifier("id"), sql.Identifier("name")]),
    table=sql.Identifier("users"),
    id=sql.Placeholder("id"),
)
# error: [column-type-mismatch]
rows = fetch_all(query, tuple[int, int])
```

## Import aliases

```python
from typing import Annotated, LiteralString
from psycopg.sql import SQL as PgSQL, Composable, Identifier
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | Composable, SQL], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
rows = fetch_all(PgSQL("SELECT id, name FROM {}").format(Identifier("users")), tuple[int, int])
```

```python
import psycopg.sql
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | psycopg.sql.Composable, SQL], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
rows = fetch_all(psycopg.sql.SQL("SELECT id, name FROM {}").format(psycopg.sql.Identifier("users")), tuple[int, int])
```

```python
import psycopg
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | psycopg.sql.Composable, SQL], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
rows = fetch_all(psycopg.sql.SQL("SELECT id FROM users"), str)
```

## Constants and Composed

A name bound once at module or function scope is followed, and so is a
module-level constant used inside `format()`.

```python
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

COLUMNS = "id, name"
USERS = sql.Identifier("users")

def users() -> None:
    query = sql.Composed([sql.SQL("SELECT " + COLUMNS + " FROM "), USERS])
    # error: [column-type-mismatch]
    fetch_all(query, tuple[int, int])
```

## Literal renders as NULL

```python
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

def by_name(name: str) -> None:
    # error: [column-type-mismatch]
    fetch_all(sql.SQL("SELECT id FROM users WHERE name = {}").format(sql.Literal(name)), str)
```

## Dynamic parts stay unchecked

```python
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

def by_column(column: str) -> list[int]:
    # error: [unchecked-query]
    return fetch_all(sql.SQL("SELECT {} FROM users").format(sql.Identifier(column)), int)
```

`Any` as the row type opts out explicitly, so it is not reported:

```python
from typing import Annotated, Any, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

def by_column(column: str) -> list[Any]:
    # error:
    return fetch_all(sql.SQL("SELECT {} FROM users").format(sql.Identifier(column)), Any)
```

A query that is no psycopg composition is skipped silently:

```python
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

def run(query: sql.Composable) -> list[int]:
    # error:
    return fetch_all(query, int)
```

## A --sql marker inside sql.SQL is not a stale rewrite

```python
from psycopg import sql

# error:
query = sql.SQL("--sql\nSELECT id, name FROM users")
```

`rewrite` leaves it alone:

```python
from psycopg import sql

query = sql.SQL("--sql\nSELECT id::INTEGER FROM users")
# rewritten: from psycopg import sql
# rewritten:
# rewritten: query = sql.SQL("--sql\nSELECT id::INTEGER FROM users")
```

## A marker in the first literal reaches the checks

```python
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
rows = fetch_all(sql.SQL("--sql\nSELECT {} FROM users").format(sql.Identifier("id")), str)
```

## Rewrite still wraps next to sql.SQL, including with an aliased wrapper

Only a string that is the first argument of a call named `...SQL` is psycopg's;
`SQL[T](...)` and a plain string are rewritten as usual:

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as PgSQL"
```

```python
from psycopg import sql

composed = sql.SQL("--sql\nSELECT id::INTEGER FROM users")
plain = "--sql\nSELECT id::INTEGER FROM users"
wrapped = PgSQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten: from psycopg import sql
# rewritten:
# rewritten: composed = sql.SQL("--sql\nSELECT id::INTEGER FROM users")
# rewritten: plain = PgSQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten: wrapped = PgSQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```
