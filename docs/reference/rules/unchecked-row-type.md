---
order: 357
group: "rewrite"
severity: "error"
fixable: false
summary: "Row type sqlty cannot check, such as a union"
origin: "Python"
---

# unchecked-row-type

Default severity: **error**

## What it does

Flags a row type sqlty cannot check against its query: a union of row types
(`SQL[A | B]`, `SQL[Union[A, B]]`, a `type Row = A | B` alias) or
`SQL[Annotated[X, ...]]`.

```python
from typing import Annotated, Literal, TypedDict

from sqlty import SQL


class Card(TypedDict):
    kind: Literal["card"]
    card_last4: str


class Iban(TypedDict):
    kind: Literal["iban"]
    iban: str


# error: [unchecked-row-type] "row type Card | Iban is a union, which sqlty cannot check against the query"
payments = SQL[Card | Iban]("--sql\nSELECT kind, card_last4, iban FROM payments")
# error: [unchecked-row-type] "row type Annotated[Card, "card"] is Annotated, which sqlty cannot check against the query"
cards = SQL[Annotated[Card, "card"]]("--sql\nSELECT kind, card_last4 FROM payments")
```

A `| None` is reported by [`optional-row`](optional-row.md) instead, and the rest
of the row type is checked. `Annotated[Any, ...]` opts out of checks, as `Any` does.
The rule reads the annotation as written: `typing.Union` and `Union` are
recognized, `t.Union` through `import typing as t` is not.

## Why is this bad?

sqlty would check nothing: a member whose fields do not match the query, or a
column that can be NULL under a `str` field, goes unreported. The psycopg
helpers cannot build such rows either, so `execute` raises `TypeError`.

## Fix

Name one row type that every row fits, such as a `TypedDict` with the fields of
both members, or split the query into one query per row type.
