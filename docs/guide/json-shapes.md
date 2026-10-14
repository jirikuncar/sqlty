# JSON shapes

A JSON column is `Any` to sqlty: the schema says `JSONB` and nothing more. A query that
*builds* the JSON says much more, and sqlty reads it. This is how a nested API response
comes out of one query and still gets checked.

The examples on this page use this schema:

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, placed TIMESTAMPTZ NOT NULL);
CREATE TABLE lines (
    id INTEGER PRIMARY KEY,
    order_id INTEGER NOT NULL REFERENCES orders (id),
    sku TEXT NOT NULL,
    qty INTEGER NOT NULL,
    note TEXT
);
```

## An endpoint that returns nested JSON

An orders endpoint returns each order with its lines. One query builds the whole document,
with no second query per order.

```python
from typing import TypedDict

from sqlty import SQL


class Line(TypedDict):
    sku: str
    qty: int
    note: str | None


class Order(TypedDict):
    id: int
    lines: list[Line]


order = SQL[Order]("""--sql
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

The `lines` column is `Any` for the type checker, but sqlty knows more:

```sql
SELECT coalesce(
    jsonb_agg(jsonb_build_object('sku', l.sku, 'qty', l.qty, 'note', l.note))
        FILTER (WHERE l.id IS NOT NULL),
    '[]'
) AS lines
FROM orders o LEFT JOIN lines l ON l.order_id = o.id
-- json: lines: [{sku: string, qty: integer, note: string | null}]
```

An array of objects: `sku` is a string, `qty` an integer, and `note` a string or `null`,
because `lines.note` has no `NOT NULL`. `coalesce(..., '[]')` keeps that element shape and
makes the column non-null. `FILTER (WHERE l.id IS NOT NULL)` drops the row a `LEFT JOIN`
makes up for an order without lines; without it, every member would be `null`-able.

## What goes wrong

The shape and the row type are written apart, so they drift. Rename a column, widen a
`NOT NULL`, change `qty` to a decimal, and the Python types still say what they said:

```python
from typing import TypedDict

from sqlty import SQL


class Line(TypedDict):
    sku: str
    qty: str
    note: str


class Order(TypedDict):
    id: int
    lines: list[Line]


# error: [json-shape-mismatch] "Order.lines is list[Line] but query column 'lines' builds JSON that does not fit: lines[].qty is str but the query builds an integer; lines[].note can be null but is declared str"
order = SQL[Order]("""--sql
SELECT
    o.id,
    coalesce(
        jsonb_agg(jsonb_build_object('sku', l.sku, 'qty', l.qty, 'note', l.note))
            FILTER (WHERE l.id IS NOT NULL),
        '[]'
    ) AS lines
FROM orders o LEFT JOIN lines l ON l.order_id = o.id
GROUP BY o.id""")
```

[`json-shape-mismatch`](../reference/rules.md#json-shape-mismatch) reports every place the
annotation cannot hold what the query builds, with the path to it (`lines[].qty`). The
nested `Line` can be a `TypedDict`; `list[...]`, `dict[str, ...]` and unions with `None` are
followed too.

## Shapes sqlty knows

| Query | Shape |
|-------|-------|
| `jsonb_build_object('a', x, 'b', y)` | object with members `a` and `b` |
| `jsonb_build_object(k, v)` (computed keys) | map: any string key, one value shape |
| `jsonb_build_array(x, y)` | array of the common shape of its items |
| `jsonb_agg(x)`, `json_agg(x)` | array of `x`, nullable if `x` can be NULL |
| `jsonb_object_agg(k, v)` | map of `v` |
| `to_jsonb(x)`, `row_to_json(t)` | the JSON of `x`; of a table row, an object of its columns |
| `'[]'::jsonb`, `'{"a": 1}'::jsonb` | the shape of the literal; `[]` has no element yet |
| `coalesce(a, b)`, `CASE`, `UNION` | `a` and `b` unified: members only some branches build are optional |
| `x -> 'key'`, `x -> 0`, `x #> '{a,b}'` | the member of `x`'s shape |

Scalars follow the SQL type: integers are `integer`, `numeric` and floats `number`,
`boolean` is `boolean`, and text, dates, UUIDs and enums are `string`. Arrays are arrays,
and a composite type is an object of its fields. An aggregate over the optional side of an
outer join has nullable members.

```sql
SELECT jsonb_build_object('id', id) AS doc FROM orders
UNION ALL
SELECT jsonb_build_object('id', id, 'note', note) FROM lines
-- json: doc: {id: integer, note?: string | null}
```

`note?` may be missing: only one branch builds it, so a `TypedDict` must declare it
`NotRequired`. `core.json_shapes()` returns the shapes of a query's columns; `-- json:` in
the examples above is that function.

## What is not checked

A shape is known only when the query builds the JSON. A JSON column read from a table
(`SELECT prefs FROM users`), `||`, `jsonb_set()`, a `->` into such a column and anything
else sqlty cannot read has no shape, and every annotation fits it. So does an annotation
of `Any`, `object` or a class sqlty cannot see into, such as an imported one or a Pydantic
model. A `dict[str, Any]` fits any object or map, but not an array.

Shapes are inferred for PostgreSQL; other dialects have none yet. `sqlty rewrite` still
writes `Any` for JSON columns: generating nested `TypedDict`s is not done.
