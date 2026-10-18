# Check arguments passed to query parameters

A marked helper's `params` argument, `*args` or `**params` carries the values of
the query's placeholders. Each argument whose type the source states is checked
against the type the query gives its parameter. An argument of unknown type is
never reported.

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    age INTEGER,
    born DATE,
    uid UUID NOT NULL DEFAULT gen_random_uuid()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## A params argument

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

ok = fetch_all("SELECT id FROM users WHERE id = %s AND name = %s", (1, "a"), int)
# error: [param-type-mismatch] "parameter 1 expects int, got bytes"
bad = fetch_all("SELECT id FROM users WHERE id = %s", (b"1",), int)
# error: [param-type-mismatch] "parameter 2 expects int, got bytes"
bytes_ = fetch_all("SELECT id FROM users WHERE name = %s AND age = %s", ["a", b"1"], int)
ok_negative = fetch_all("SELECT id FROM users WHERE id = %s", (-1,), int)
negative_float = fetch_all("SELECT id FROM users WHERE id = %s", (-b"1",), int)
```

### Text is accepted for any type

The server coerces text, so a `str` is never a mismatch.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

ok = fetch_all("SELECT id FROM users WHERE id = %s AND born = %s", ("1", "2020-01-01"), int)
# error:
```

### Numbers are interchangeable

```python
from decimal import Decimal
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

ok = fetch_all("SELECT id FROM users WHERE id = %s AND age > %s", (1.5, Decimal("2")), int)
ok_limit = fetch_all("SELECT id FROM users LIMIT %s", (2.0,), int)
# error:
```

### Booleans are not numbers in PostgreSQL

PostgreSQL has no implicit cast between `boolean` and the number types, and the
drivers send `True` as a `boolean`, so `age = %s` with `True` fails on the
server.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

# error: [param-type-mismatch] "parameter 1 expects int, got bool"
age = fetch_all("SELECT id FROM users WHERE age = %s", (True,), int)
# error: [param-type-mismatch] "parameter 1 expects bool, got int"
flag = fetch_all("SELECT id FROM users WHERE (age > 1) = %s", (1,), int)
ok = fetch_all("SELECT id FROM users WHERE (age > 1) = %s", (False,), int)
```

#### MySQL and SQLite take either

They read `TRUE` as `1`.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

# error:
age = fetch_all("SELECT id FROM users WHERE age = %s", (True,), int)
```

### An explicit cast takes any argument

The server converts the value, so only `None` for a `NOT NULL` target is reported.

```python
from datetime import date
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

# error: [column-count-mismatch] "str takes 1 column but query returns 3 columns"
ok = fetch_all("SELECT %s::text, CAST(%s AS int), %s::text[]", (1, date(2020, 1, 1), (1, 2)), str)
```

### Compatible numbers and dates

```python
from datetime import date, datetime
from decimal import Decimal
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

ok = fetch_all("SELECT id FROM users WHERE id = %s AND born = %s", (1, datetime.now()), int)
# error: [param-type-mismatch] "parameter 2 expects datetime.date, got decimal.Decimal"
bad = fetch_all("SELECT id FROM users WHERE id = %s AND born = %s", (1, Decimal("1")), int)
```

## None for a NOT NULL column

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

execute("INSERT INTO users (id, name, age) VALUES (%s, %s, %s)", (1, "a", None))
# error: [param-type-mismatch] "parameter 2 is not nullable, got None"
execute("INSERT INTO users (id, name, age) VALUES (%s, %s, %s)", (1, None, 3))
```

A comparison does not say whether `None` is valid, so it is not reported:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

execute("DELETE FROM users WHERE age = %s", (None,))
# error:
```

## Annotated variables and constructors

```python
from datetime import date
from typing import Annotated, LiteralString
from uuid import UUID
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

def lookup(uid: UUID, born: date, age: bytes, other):
    execute("DELETE FROM users WHERE uid = %s AND born = %s", (uid, born))
    execute("DELETE FROM users WHERE uid = %s", (other,))
    # error: [param-type-mismatch] "parameter 1 expects int, got bytes"
    execute("DELETE FROM users WHERE age = %s", (age,))
    # error: [param-type-mismatch] "parameter 1 expects uuid.UUID, got datetime.date"
    execute("DELETE FROM users WHERE uid = %s", (born,))
```

## Arrays

`= ANY(%s)` takes a list.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

ids: list[int] = [1, 2]
execute("DELETE FROM users WHERE id = ANY(%s)", ([1, 2],))
execute("DELETE FROM users WHERE id = ANY(%s)", (ids,))
# error: [param-type-mismatch] "parameter 1 expects list[int], got int"
execute("DELETE FROM users WHERE id = ANY(%s)", (1,))
# error: [param-type-mismatch] "parameter 1 expects list[int], got list[bytes]"
execute("DELETE FROM users WHERE id = ANY(%s)", ([b"1"],))
```

## Named parameters

A dict literal, or `**params` keywords, binds `%(name)s` placeholders.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: dict[str, object], /) -> None: ...
def run(query: Annotated[LiteralString, SQL], /, **params: object) -> None: ...

execute("DELETE FROM users WHERE id = %(id)s", {"id": 1})
# error: [param-type-mismatch] "parameter 'id' expects int, got bytes"
execute("DELETE FROM users WHERE id = %(id)s", {"id": b"1"})
run("DELETE FROM users WHERE id = %(id)s AND name = %(name)s", id=1, name="a")
# error: [param-type-mismatch] "parameter 'id' expects int, got bytes"
run("DELETE FROM users WHERE id = %(id)s", id=b"1")
```

## asyncpg style *args

```python
from typing import Annotated, LiteralString
from sqlty import SQL

async def fetch(query: Annotated[LiteralString, SQL], *args: object, timeout: float | None = None) -> list[object]: ...

async def main() -> None:
    await fetch("SELECT id FROM users WHERE id = $1 AND name = $2", 1, "a", timeout=3)
    # error: [param-type-mismatch] "parameter 2 expects str, got int"
    await fetch("SELECT id FROM users WHERE id = $1 AND name = $2", 1, 2, timeout=3)
```

## Unknown argument types are skipped

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

def f(x, y: "list[int]", *rest):
    execute("DELETE FROM users WHERE id = %s AND age = %s", (x, y))
    execute("DELETE FROM users WHERE id = %s AND age = %s", (*rest,))
    execute("DELETE FROM users WHERE id = %s", tuple(rest))
# error:
```

## A call without the params is not checked

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...] = ()) -> None: ...

execute("DELETE FROM users WHERE id = %s")
# error:
```

## Rule configuration

```toml
[tool.sqlty.rules]
param-type-mismatch = "ignore"
```

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

execute("DELETE FROM users WHERE id = %s", (b"1",))
# error:
```
