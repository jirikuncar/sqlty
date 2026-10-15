# json-shape-mismatch

Default severity: **error**

## What it does

Reports a field of a user-defined row type (or an item of a helper's `tuple[...]` row
type) that cannot hold the JSON its query column builds. The column must be built in the
query, with `jsonb_build_object()`, `jsonb_agg()`, `to_jsonb()` and the like (PostgreSQL),
so that sqlty knows its shape; the annotation is followed through nested `TypedDict`s,
`list[...]` and `dict[str, ...]`. A scalar of the wrong type, a member that can be `null`
while the annotation is not optional, a required key the query may omit or never builds,
and a key the `TypedDict` does not declare are all reported, each with its path. A nested
Pydantic model is checked for its keys only (by alias, extra keys allowed by
`extra="allow"`), not for the types of its fields.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY);
CREATE TABLE lines (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL, sku TEXT NOT NULL, qty INTEGER NOT NULL);
```

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

## Why is this bad?

The JSON column is `Any` to a type checker, so nothing connects the query that builds the
document to the `TypedDict` that describes it. The annotation promises `qty: str`; the
database sends an integer.

## How to fix

Change the annotation to what the query builds (`qty: int`), or change the query. Where a
member is optional, use `NotRequired[...]`; where it can be `null`, use `... | None`.

```python
from typing import TypedDict

from sqlty import SQL


class Line(TypedDict):
    sku: str
    qty: int


class Order(TypedDict):
    id: int
    lines: list[Line]


q = SQL[Order]("""--sql
SELECT o.id, (SELECT coalesce(jsonb_agg(jsonb_build_object('sku', l.sku, 'qty', l.qty)), '[]') FROM lines l WHERE l.order_id = o.id) AS lines
FROM orders o""")
# error:
```

A column of unknown shape (JSON read from a table, `||`, `jsonb_set()`), and a field
annotated `Any`, `object`, or with a class sqlty cannot see into, are never reported. The
JSON shapes guide lists the shapes sqlty infers.

## Configuration

To lower the severity or turn the rule off:

```toml
[tool.sqlty.rules]
json-shape-mismatch = "warn"
```
