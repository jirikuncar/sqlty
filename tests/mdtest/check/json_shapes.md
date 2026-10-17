# json-shape-mismatch

A column built with `jsonb_build_object()`, `jsonb_agg()` and friends has a known JSON
shape (see `tests/mdtest/postgres/json_shapes.md`). `json-shape-mismatch` checks it against
the annotation of the row field that receives it: nested TypedDicts, `list[...]` and
`dict[str, ...]`.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total NUMERIC(10, 2) NOT NULL);
CREATE TABLE lines (
    id INTEGER PRIMARY KEY,
    order_id INTEGER NOT NULL,
    sku TEXT NOT NULL,
    qty INTEGER NOT NULL,
    note TEXT
);
```

## Nested TypedDicts that fit

```python
from typing import NotRequired, TypedDict

from sqlty import SQL


class Line(TypedDict):
    sku: str
    qty: int
    note: str | None


class Order(TypedDict):
    id: int
    lines: list[Line]


q = SQL[Order]("""--sql
SELECT
    o.id,
    coalesce(
        jsonb_agg(jsonb_build_object('sku', l.sku, 'qty', l.qty, 'note', l.note) ORDER BY l.id)
            FILTER (WHERE l.id IS NOT NULL),
        '[]'
    ) AS lines
FROM orders o LEFT JOIN lines l ON l.order_id = o.id
GROUP BY o.id""")
# error:
```

## A scalar of the wrong type

```python
from typing import TypedDict

from sqlty import SQL


class Line(TypedDict):
    sku: str
    qty: str


class Order(TypedDict):
    id: int
    lines: list[Line]


# error: [json-shape-mismatch] "Order.lines is list[Line] but query column 'lines' builds JSON that does not fit: lines[].qty is str but the query builds an integer"
q = SQL[Order]("""--sql
SELECT o.id, (SELECT coalesce(jsonb_agg(jsonb_build_object('sku', l.sku, 'qty', l.qty)), '[]') FROM lines l WHERE l.order_id = o.id) AS lines
FROM orders o""")
```

## A number fits int, float and Decimal, not str

A numeric value is an integer in JSON when it is integral, so Python reads an `int` or a
`float`; `Decimal` fits too.

```python
from decimal import Decimal
from typing import TypedDict

from sqlty import SQL


class Summary(TypedDict):
    id: int
    a: int
    b: float
    c: Decimal
    d: str


# error: [json-shape-mismatch] "Summary.d is str but query column 'd' builds JSON"
q = SQL[Summary]("""--sql
SELECT id, to_jsonb(total) AS a, to_jsonb(total) AS b, to_jsonb(total) AS c, to_jsonb(total) AS d
FROM orders""")
```

## A member that can be null

A `LEFT JOIN` null-extends the row, so every member of `jsonb_agg(jsonb_build_object(...))`
can be `null`.

```python
from typing import TypedDict

from sqlty import SQL


class Line(TypedDict):
    sku: str


class Order(TypedDict):
    id: int
    lines: list[Line]


# error: [json-shape-mismatch] "Order.lines is list[Line] but query column 'lines' builds JSON that does not fit: lines[].sku can be null but is declared str"
q = SQL[Order]("""--sql
SELECT o.id, jsonb_agg(jsonb_build_object('sku', l.sku)) AS lines
FROM orders o LEFT JOIN lines l ON l.order_id = o.id
GROUP BY o.id""")
```

```python
from typing import TypedDict

from sqlty import SQL


class Line(TypedDict):
    note: str | None


class Order(TypedDict):
    lines: list[Line]


q = SQL[Order]("--sql\nSELECT coalesce(jsonb_agg(jsonb_build_object('note', note)), '[]') AS lines FROM lines")
# error:
```

## Elements that can be null

A helper's `tuple[...]` row type is checked item by item.

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T: ...


# error: [json-shape-mismatch] "tuple[list[str]] item 0 is list[str] but query column 'notes' builds JSON that does not fit: notes[] can be null but is declared str"
a = fetch_one("SELECT coalesce(jsonb_agg(note), '[]') AS notes FROM lines", tuple[list[str]])
b = fetch_one("SELECT coalesce(jsonb_agg(note), '[]') AS notes FROM lines", tuple[list[str | None]])
```

## Required and optional keys

An object that only some branches build has optional members; a required key must be
built every time. A key the query never builds is missing.

```python
from typing import NotRequired, TypedDict

from sqlty import SQL


class Strict(TypedDict):
    id: int
    extra: str


class Loose(TypedDict):
    id: int
    extra: NotRequired[str]


class Row(TypedDict):
    strict: Strict
    loose: Loose


# error: [json-shape-mismatch] "Row.strict is Strict but query column 'strict' builds JSON that does not fit: strict.extra is required by Strict but the query may omit it"
q = SQL[Row]("""--sql
SELECT
    CASE WHEN total > 0 THEN jsonb_build_object('id', id, 'extra', 'x') ELSE jsonb_build_object('id', id) END AS strict,
    CASE WHEN total > 0 THEN jsonb_build_object('id', id, 'extra', 'x') ELSE jsonb_build_object('id', id) END AS loose
FROM orders""")
```

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: int
    name: str


class Row(TypedDict):
    doc: Doc


# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc.name is required by Doc but the query does not build it"
q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id) AS doc FROM orders")
```

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: int


class Row(TypedDict):
    doc: Doc


# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc has key 'total' that Doc does not declare"
q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id, 'total', total) AS doc FROM orders")
```

## Required and optional keys together

An optional key the query never builds is fine; a required key it never builds is not,
whatever the optional ones do.

```python
from typing import NotRequired, TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: int
    name: str
    note: NotRequired[str]
    tag: NotRequired[str]


class Row(TypedDict):
    doc: Doc


q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id, 'name', 'x') AS doc FROM orders")
# error:
```

```python
from typing import NotRequired, TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: int
    name: str
    note: NotRequired[str]


class Row(TypedDict):
    doc: Doc


# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc.name is required by Doc but the query does not build it"
q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id, 'note', 'x') AS doc FROM orders")
```

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: int
    name: str
    total: float


class Row(TypedDict):
    doc: Doc


# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc.name is required by Doc but the query does not build it; doc.total is required by Doc but the query does not build it"
q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id) AS doc FROM orders")
```

## Keys the type does not declare

`extra_items=T` and `dict[str, T]` accept further keys; a plain or closed `TypedDict` does not.

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict, closed=True):
    id: int


class Row(TypedDict):
    doc: Doc


# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc has key 'total' that Doc does not declare"
q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id, 'total', total) AS doc FROM orders")
```

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict, extra_items=float):
    id: int


class Row(TypedDict):
    doc: Doc
    free: dict[str, float]


q = SQL[Row]("""--sql
SELECT jsonb_build_object('id', id, 'total', total) AS doc, jsonb_build_object('a', total, 'b', total) AS free
FROM orders""")
# error:
```

## total=False

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict, total=False):
    id: int
    total: float


class Row(TypedDict):
    doc: Doc


q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id) AS doc FROM orders")
# error:
```

## Maps

`jsonb_object_agg()` builds an object with arbitrary keys: a `dict[str, ...]`.

```python
from typing import TypedDict

from sqlty import SQL


class Row(TypedDict):
    totals: dict[str, float]
    by_user: dict[str, str]


# error: [json-shape-mismatch] "Row.by_user is dict[str, str] but query column 'by_user' builds JSON that does not fit: by_user[key] is str but the query builds a number"
q = SQL[Row]("""--sql
SELECT
    coalesce(jsonb_object_agg(id::TEXT, total), '{}') AS totals,
    coalesce(jsonb_object_agg(user_id::TEXT, total), '{}') AS by_user
FROM orders""")
```

A map is not a TypedDict, and an array is not a `dict`.

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: int


class Row(TypedDict):
    ids: dict[str, int]
    doc: Doc


# error: [json-shape-mismatch] "Row.ids is dict[str, int] but query column 'ids' builds JSON that does not fit: ids is dict[str, int] but the query builds an array"
# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc is Doc but the query builds a map"
q = SQL[Row]("""--sql
SELECT coalesce(jsonb_agg(id), '[]') AS ids, coalesce(jsonb_object_agg(id::TEXT, id), '{}') AS doc FROM orders""")
```

## Annotations that always fit

`Any`, `object` and classes sqlty cannot see into accept any shape; so does a JSON column of
unknown shape.

```python
from typing import Any, TypedDict

from sqlty import SQL


class Row(TypedDict):
    a: Any
    b: object
    c: dict[str, Any]
    d: list[Any]
    e: Any | None


q = SQL[Row]("""--sql
SELECT
    jsonb_build_object('id', id) AS a,
    jsonb_agg(id) AS b,
    jsonb_build_object('id', id, 'n', jsonb_agg(total)) AS c,
    jsonb_agg(jsonb_build_object('id', id)) AS d,
    to_jsonb(total) AS e
FROM orders GROUP BY id, total""")
# error:
```

## Unions and optional columns

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: int


class Row(TypedDict):
    doc: Doc | None
    docs: list[Doc] | Doc


q = SQL[Row]("""--sql
SELECT jsonb_build_object('id', id) AS doc, jsonb_agg(jsonb_build_object('id', id)) AS docs
FROM orders GROUP BY id""")
# error:
```

## Dataclasses

```python
from dataclasses import dataclass
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: str


@dataclass
class Row:
    doc: Doc


# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc.id is str but the query builds an integer"
q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id) AS doc FROM orders")
```

## Configuring the rule

```toml
[tool.sqlty.rules]
json-shape-mismatch = "ignore"
```

```python
from typing import TypedDict

from sqlty import SQL


class Doc(TypedDict):
    id: str


class Row(TypedDict):
    doc: Doc


q = SQL[Row]("--sql\nSELECT jsonb_build_object('id', id) AS doc FROM orders")
# error:
```
