# redundant-literal

Default severity: **ignore**

## What it does

Reports a field of a user-defined row type (or an item of a helper's `tuple[...]` row
type) annotated with a `Literal` that lists values its query column can never take. This
is the strict side of [`literal-mismatch`](literal-mismatch.md): that rule finds missing
values, this one finds extra ones.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-literal = "warn"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid'))
);
```

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    id: int
    status: Literal["pending", "paid", "refunded"]


# error: [redundant-literal] "Order.status is Literal['pending', 'paid', 'refunded'] but query column 'status' is never 'refunded'"
q = SQL[Order]("--sql\nSELECT id, status FROM orders")
```

## Why is this bad?

Callers handle a case that never arrives, and the annotation hides which values the
query really returns.

## How to fix

Drop the extra values, or accept them: a wider `Literal` is correct, just looser than
needed. That is why the rule is off by default: one shared `Literal` alias for a status
column is used by many queries, and each of them returns only some of its values.

```python
from typing import Literal, TypedDict

from sqlty import SQL


class Order(TypedDict):
    id: int
    status: Literal["pending", "paid"]


q = SQL[Order]("--sql\nSELECT id, status FROM orders")
# error:
```
