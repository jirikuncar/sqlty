# Literal annotations against value sets

A field annotated with a `Literal` is compared with the values its query column can take.
`literal-mismatch` (error) reports values the annotation misses; `redundant-literal`
(off by default) reports values it lists that the column never takes. A field that takes
values by class (`str`, `int`, `Any`, an enum class) is never reported.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');
CREATE DOMAIN grade AS integer CHECK (VALUE IN (1, 2, 3));
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'refunded')),
    priority INTEGER CHECK (priority IN (1, 2, 3)),
    mood mood NOT NULL,
    grade grade NOT NULL,
    note TEXT
);
```

## A Literal that misses values

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    id: int
    status: Literal["pending", "paid"]


# error: [literal-mismatch] "Order.status is Literal['pending', 'paid'] but query column 'status' can be 'refunded'"
q = SQL[Order]("--sql\nSELECT id, status FROM orders")
```

## A Literal that covers the values

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    id: int
    status: Literal["pending", "paid", "refunded"]


q = SQL[Order]("--sql\nSELECT id, status FROM orders")
# error:
```

## Nullable column

`None` in the annotation is `nullable-mismatch`'s business, not this rule's.

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    priority: Literal[1, 2, 3] | None
    note: Literal["a"] | None


q = SQL[Order]("--sql\nSELECT priority, note FROM orders")
# error:
```

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    priority: Literal[1, 2]


# error: [literal-mismatch] "Order.priority is Literal[1, 2] but query column 'priority' can be 3"
# error: [nullable-mismatch] "Order.priority is Literal[1, 2] but query column 'priority' can be NULL"
q = SQL[Order]("--sql\nSELECT priority FROM orders")
```

## Enums and domains

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Person(TypedDict):
    mood: Literal["sad", "ok"]
    grade: Literal[1, 2]


# error: [literal-mismatch] "Person.mood is Literal['sad', 'ok'] but query column 'mood' can be 'happy'"
# error: [literal-mismatch] "Person.grade is Literal[1, 2] but query column 'grade' can be 3"
q = SQL[Person]("--sql\nSELECT mood, grade FROM orders")
```

## Constants, CASE and UNION

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Row(TypedDict):
    kind: Literal["a", "b"]
    size: Literal["s", "m"]
    state: Literal["pending", "paid"]


# error: [literal-mismatch] "Row.size is Literal['s', 'm'] but query column 'size' can be 'l'"
# error: [literal-mismatch] "Row.state is Literal['pending', 'paid'] but query column 'state' can be 'refunded', 'void'"
q = SQL[Row](
    "--sql\nSELECT 'a' AS kind, CASE WHEN id > 1 THEN 's' WHEN id > 0 THEN 'm' ELSE 'l' END AS size, "
    "status AS state FROM orders UNION ALL SELECT 'b', 's', 'void' FROM orders"
)
```

## Fields that take values by class

```python
import enum
from typing import Any, Literal, TypedDict

from sqlty import SQL


class Status(enum.StrEnum):
    PENDING = "pending"


class Order(TypedDict):
    a: str
    b: Any
    c: object
    d: Status
    e: str | Literal["x"]
    f: int | None


q = SQL[Order](
    "--sql\nSELECT status AS a, status AS b, status AS c, status AS d, status AS e, priority AS f "
    "FROM orders"
)
# error:
```

## An unknown column is never reported

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    name: Literal["a"]
    note: Literal["a"] | None


q = SQL[Order]("--sql\nSELECT id::TEXT AS name, note FROM orders")
# error:
```

## Helper row types

```python
from typing import Annotated, Literal, LiteralString

from sqlty import SQL


def fetch[T](query: Annotated[LiteralString, SQL], row: type[T]) -> list[T]:
    raise NotImplementedError


# error: [literal-mismatch] "tuple[int, Literal['pending']] item 1 is Literal['pending'] but query column 'status' can be 'paid', 'refunded'"
rows = fetch("SELECT id, status FROM orders", tuple[int, Literal["pending"]])
rows = fetch("SELECT id, status FROM orders", tuple[int, Literal["pending", "paid", "refunded"]])
rows = fetch("SELECT id, status FROM orders", tuple[int, str])
```

## redundant-literal

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-literal = "warn"
```

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    status: Literal["pending", "paid", "refunded", "void"]
    mood: Literal["sad", "ok", "happy"]


# error: [redundant-literal] "Order.status is Literal['pending', 'paid', 'refunded', 'void'] but query column 'status' is never 'void'"
q = SQL[Order]("--sql\nSELECT status, mood FROM orders")
```

A `Literal` that misses a value is `literal-mismatch`, not `redundant-literal`, even when
it lists another one that cannot occur.

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    status: Literal["pending", "void"]


# error: [literal-mismatch] "Order.status is Literal['pending', 'void'] but query column 'status' can be 'paid', 'refunded'"
q = SQL[Order]("--sql\nSELECT status FROM orders")
```

## Severity

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
literal-mismatch = "ignore"
```

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    status: Literal["pending"]


q = SQL[Order]("--sql\nSELECT status FROM orders")
# error:
```

## SQL[tuple[...]] rows

A `Literal` item of a `SQL[tuple[...]]` wrapper is compared with the inferred values by set,
so quotes, order, the module spelling and repeats do not make it stale.

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from typing import Literal

from sqlty import SQL

a = SQL[tuple[int, Literal["pending", "paid", "refunded"]]]("--sql\nSELECT id, status FROM orders")
b = SQL[tuple[int, Literal['refunded', 'paid', 'pending', 'paid']]]("--sql\nSELECT id, status FROM orders")
c = SQL[tuple[int, typing.Literal["paid", "pending", "refunded"]]]("--sql\nSELECT id, status FROM orders")
d = SQL[tuple[Literal[1, 2, 3] | None]]("--sql\nSELECT priority FROM orders")
# error:
```

`check --fix` leaves them alone.

```python
from typing import Literal

from sqlty import SQL

a = SQL[tuple[Literal["paid", "pending", "refunded"]]]("--sql\nSELECT status FROM orders")
# fixed: from typing import Literal
# fixed:
# fixed: from sqlty import SQL
# fixed:
# fixed: a = SQL[tuple[Literal["paid", "pending", "refunded"]]]("--sql\nSELECT status FROM orders")
```

A `Literal` that misses a value is `literal-mismatch`, not `stale-type`.

```python
from typing import Literal

from sqlty import SQL

# error: [literal-mismatch] "but its query column can be 'paid', 'refunded'"
q = SQL[tuple[int, Literal["pending"]]]("--sql\nSELECT id, status FROM orders")
```

One that differs in `None` is stale.

```python
from typing import Literal

from sqlty import SQL

# error: [stale-type] "should be SQL[tuple[typing.Literal[1, 2, 3] | None]]"
q = SQL[tuple[Literal[1, 2, 3]]]("--sql\nSELECT priority FROM orders")
```

`redundant-literal`, when enabled, reports values the column never takes.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-literal = "warn"
```

```python
from typing import Literal

from sqlty import SQL

# error: [redundant-literal] "but its query column is never 'void'"
q = SQL[tuple[Literal["pending", "paid", "refunded", "void"]]]("--sql\nSELECT status FROM orders")
```
