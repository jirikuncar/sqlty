# Parameters compared with Literal columns

A column with a value set (a `CHECK ... IN`, an enum, a constant) gives its parameters the
type `str` or `int`, not a `Literal`: the check cannot know which value a caller passes,
and a `str` is always accepted.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid')),
    priority INTEGER NOT NULL CHECK (priority IN (1, 2, 3))
);
```

## Inferred type

```sql
SELECT id FROM orders WHERE status = %s AND priority = %s AND status = ANY(%s)
-- params: 1: str (TEXT)
-- params: 2: int (INT)
-- params: 3: list[str] (ARRAY<TEXT>)
```

## An arbitrary str argument is accepted

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: tuple[object, ...], form: type[T], /) -> list[T]: ...

def by_status(status: str, priority: int) -> list[int]:
    return fetch_all("SELECT id FROM orders WHERE status = %s AND priority = %s", (status, priority), int)

known = fetch_all("SELECT id FROM orders WHERE status = %s", ("paid",), int)
unlisted = fetch_all("SELECT id FROM orders WHERE status = %s", ("unknown",), int)
# error: [param-type-mismatch] "parameter 1 expects str, got bytes"
wrong = fetch_all("SELECT id FROM orders WHERE status = %s", (b"paid",), int)
```

## Arguments of no checked type

A complex number, a list of only `None` and a dict with a key that is not a string give
nothing to compare; a negated `bool` is an `int` and a set is a list.

```python
from typing import Annotated, Any, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], params: Any, form: type[T], /) -> list[T]: ...

a = fetch_all("SELECT id FROM orders WHERE priority = %s", (1j,), int)
b = fetch_all("SELECT id FROM orders WHERE priority = %s", (-1j,), int)
c = fetch_all("SELECT id FROM orders WHERE priority = %s", (-True,), int)
d = fetch_all("SELECT id FROM orders WHERE priority = ANY(%s)", ({1, 2},), int)
e = fetch_all("SELECT id FROM orders WHERE priority = ANY(%s)", ([None],), int)
f = fetch_all("SELECT id FROM orders WHERE priority = %(p)s", {1: 2}, int)
# error:
```

## A `Literal` parameter type outside the column's values

`P` of `SQL[R, P]` that annotates a parameter `Literal[...]` with a value its column never
holds is reported: the database rejects the write, or the comparison matches no row. A
`Literal` that keeps one value of the column, and a `str`, are accepted.

```python
from typing import Literal, TypedDict
from sqlty import SQL

class Refund(TypedDict):
    status: Literal["paid", "refunded"]

# error: [params-type-mismatch] "tuple[Literal['refunded'], int] item 1 is Literal['refunded'] but query parameter 1 is never 'refunded'"
a = SQL[tuple[int], tuple[Literal["refunded"], int]]("--sql\nSELECT id FROM orders WHERE status = %s AND priority = %s")
# error: [params-type-mismatch] "tuple[str, Literal[3, 4] | None] item 2 is Literal[3, 4] | None but query parameter 2 is never 4"
# error: [params-type-mismatch] "tuple[str, Literal[3, 4] | None] item 2 is Literal[3, 4] | None but query parameter 2 is not nullable"
b = SQL[tuple[()], tuple[str, Literal[3, 4] | None]]("--sql\nUPDATE orders SET status = %s, priority = %s WHERE id = 1")
# error: [params-type-mismatch] "Refund.status is Literal['paid', 'refunded'] but query parameter 'status' is never 'refunded'"
c = SQL[tuple[int], Refund]("--sql\nSELECT id FROM orders WHERE status = %(status)s")
d = SQL[tuple[int], tuple[Literal["paid", "pending"], Literal[1] | None, str]]("--sql\nSELECT id FROM orders WHERE status = %s AND priority = %s AND status = %s")
```

### Only where the column limits the parameter

A parameter also compared with a constant, or by another operator than `=` and `IN`, may
hold other values: it is not judged.

```python
from typing import Literal, TypedDict
from sqlty import SQL

class Filter(TypedDict):
    f: Literal["all", "paid"]

# error:
a = SQL[tuple[int], Filter]("--sql\nSELECT id FROM orders WHERE (%(f)s = 'all' OR status = %(f)s)")
b = SQL[tuple[int], tuple[Literal["m"]]]("--sql\nSELECT id FROM orders WHERE status > %s")
c = SQL[tuple[int], tuple[Literal["x"]]]("--sql\nSELECT id FROM orders WHERE status <> %s")
d = SQL[tuple[str], tuple[Literal["unknown"]]]("--sql\nSELECT COALESCE(status, %s) FROM orders")
```

### A TypedDict of another module

`pkg/models.py`:

```python
from typing import Literal, TypedDict


class Refund(TypedDict):
    status: Literal["paid", "refunded"]
```

```python
from sqlty import SQL
from pkg.models import Refund

# error: [params-type-mismatch] "Refund.status is Literal['paid', 'refunded'] but query parameter 'status' is never 'refunded'"
c = SQL[tuple[int], Refund]("--sql\nSELECT id FROM orders WHERE status = %(status)s")
```

### An enum

```toml
[tool.sqlty]
schema = "enum.sql"
```

`enum.sql`:

```sql
CREATE TYPE mood AS ENUM ('sad', 'ok');
CREATE TABLE people (id INTEGER PRIMARY KEY, current mood NOT NULL);
```

```python
from typing import Literal
from sqlty import SQL

# error: [params-type-mismatch] "tuple[Literal['happy']] item 1 is Literal['happy'] but query parameter 1 is never 'happy'"
a = SQL[tuple[int], tuple[Literal["happy"]]]("--sql\nSELECT id FROM people WHERE current = %s")
```

### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "sqlite.sql"
```

`sqlite.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid')));
```

```python
from typing import Literal
from sqlty import SQL

# error: [params-type-mismatch] "tuple[Literal['refunded']] item 1 is Literal['refunded'] but query parameter 1 is never 'refunded'"
a = SQL[tuple[()], tuple[Literal["refunded"]]]("--sql\nUPDATE orders SET status = ? WHERE id = 1")
```

### MySQL is not judged

```toml
[tool.sqlty]
dialect = "mysql"
schema = "mysql.sql"
```

`mysql.sql`:

```sql
CREATE TABLE orders (id INT PRIMARY KEY, status ENUM('pending', 'paid') NOT NULL);
```

```python
from typing import Literal
from sqlty import SQL

# error:
a = SQL[tuple[int], tuple[Literal["PAID"]]]("-- sql\nSELECT id FROM orders WHERE status = %s")
```
