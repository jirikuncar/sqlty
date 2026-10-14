# literal-mismatch

Default severity: **error**

## What it does

Reports a field of a user-defined row type (or an item of a helper's `tuple[...]` row
type) annotated with a `Literal` that does not cover every value its query column can
take. sqlty knows the values of a column from an enum, a `CHECK (col IN (...))` or
`CHECK (col = ANY (ARRAY[...]))` constraint (also through a domain), a `CASE` or
`COALESCE` over constants, a `UNION` of such columns, and a constant itself.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'refunded'))
);
```

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    id: int
    status: Literal["pending", "paid"]


# error: [literal-mismatch] "Order.status is Literal['pending', 'paid'] but query column 'status' can be 'refunded'"
q = SQL[Order]("--sql\nSELECT id, status FROM orders")
```

## Why is this bad?

The annotation promises values the database may not return. A type checker narrows on
`status == "paid"` and treats a `"refunded"` row as impossible.

## How to fix

Add the missing values to the `Literal`, or narrow the query so it cannot return them
(`WHERE status IN ('pending', 'paid')` does not narrow the inferred values yet, so list
them in the annotation or use `str`).

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    id: int
    status: Literal["pending", "paid", "refunded"]


q = SQL[Order]("--sql\nSELECT id, status FROM orders")
# error:
```

An annotation that takes values by class is never reported: `str` (or `int` for an
integer column), `Any`, `object`, or an enum class. A column whose values are unknown,
such as `name::TEXT` or any column without a schema, is never reported either: sqlty
only reports what the schema or the query proves. A set larger than 32 values is not
tracked and the column is plain `str` or `int`.

## Configuration

The rule that finds the opposite, `Literal` values the column can never take, is
[`redundant-literal`](redundant-literal.md); it is off by default. To lower the severity
of this rule or turn it off:

```toml
[tool.sqlty.rules]
literal-mismatch = "warn"
```
