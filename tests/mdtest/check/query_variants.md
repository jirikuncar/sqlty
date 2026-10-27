# Query variants from branches

Acceptance spec of [the query variants plan](../../../plans/2026-10-27-query-variants.md). A query built
from names bound in branches is checked once per variant. Sections marked `mdtest:xfail`
are strict: remove the marker when the section passes, and change nothing else
(`tests/test_query_variants_spec.py`).

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
dynamic-query = "warn"
```

`schema.sql`:

```sql
CREATE TABLE foos (id INTEGER PRIMARY KEY, foo TEXT, n INTEGER NOT NULL);
```

`db.py`:

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...


def fetch_params[T](query: Annotated[LiteralString, SQL], form: type[T], /, *params: object) -> list[T]: ...
```

## if/else

```python
from typing import LiteralString

from db import fetch_all


def find(foo: str | None) -> list[tuple[int, str | None]]:
    cond: LiteralString
    if foo is None:
        cond = "foo IS NULL"
    else:
        cond = "foo IS NOT NULL"
    # error:
    return fetch_all("SELECT id, foo FROM foos WHERE " + cond, tuple[int, str | None])
```

## A mismatch in one variant names it

```python
from typing import LiteralString

from db import fetch_all


def find(by_n: bool) -> list[tuple[int]]:
    column: LiteralString
    if by_n:
        column = "n"
    else:
        column = "foo"
    # error: [column-type-mismatch] "(variant: line 11)"
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## A mismatch in every variant is reported once, without a variant

```python
from typing import LiteralString

from db import fetch_all


def find(flag: bool) -> list[tuple[str]]:
    cond: LiteralString = "id = 1" if flag else "id = 2"
    # error: [column-type-mismatch] "infers int"
    return fetch_all("SELECT n FROM foos WHERE " + cond, tuple[str])
```

## elif and nested branches

```python
from typing import LiteralString

from db import fetch_all


def find(a: int, b: bool) -> list[tuple[int]]:
    column: LiteralString
    if a == 1:
        column = "id"
    elif a == 2:
        if b:
            column = "n"
        else:
            column = "id + n"
    else:
        column = "foo"
    # error: [column-type-mismatch] "(variant: line 16)"
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## match/case

```python
from typing import LiteralString

from db import fetch_all


def find(kind: str) -> list[tuple[int]]:
    column: LiteralString
    match kind:
        case "id":
            column = "id"
        case "n":
            column = "n"
        case _:
            raise ValueError(kind)
    # error:
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## Names bound in one branch stay together

Two branches give two variants, not four: `SELECT n FROM (SELECT 1 AS k) t` never runs.

```python
from typing import LiteralString

from db import fetch_all


def find(inline: bool) -> list[tuple[int]]:
    column: LiteralString
    source: LiteralString
    if inline:
        column, source = "k", "(SELECT 1 AS k) t"
    else:
        column = "n"
        source = "foos"
    # error:
    return fetch_all("SELECT " + column + " FROM " + source, tuple[int])
```

## A condition tested twice takes the same branch

```python
from typing import LiteralString

from db import fetch_all


def find(inline: bool) -> list[tuple[int]]:
    column: LiteralString = "k" if inline else "n"
    source: LiteralString = "(SELECT 1 AS k) t" if inline else "foos"
    # error:
    return fetch_all("SELECT " + column + " FROM " + source, tuple[int])
```

## A branch that returns or raises adds no variant

```python
from typing import LiteralString

from db import fetch_all


def find(foo: str | None) -> list[tuple[int]]:
    column: LiteralString = "id"
    if foo is None:
        column = "foo"
        raise ValueError("no foo")
    # error:
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## Augmented assignment

```python
from db import fetch_params


def find(foo: str | None, n: int | None) -> list[tuple[int]]:
    query = "SELECT id FROM foos WHERE TRUE"
    params: list[object] = []
    if foo is not None:
        query += " AND foo = %s"
        params.append(foo)
    if n is not None:
        query += " AND n = %s"
        params.append(n)
    # error:
    return fetch_params(query, tuple[int], *params)
```

## A Literal parameter

```python
from typing import Literal

from db import fetch_all


def find(column: Literal["id", "foo"]) -> list[tuple[int]]:
    # error: [column-type-mismatch] "(variant: line 6)"
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## A function that returns a fragment

```python
from typing import LiteralString

from db import fetch_all


def where(foo: str | None) -> LiteralString:
    if foo is None:
        return "foo IS NULL"
    return "foo IS NOT NULL"


def find(foo: str | None) -> list[tuple[int, str]]:
    # error: [nullable-mismatch] "(variant: line 8)"
    return fetch_all("SELECT id, foo FROM foos WHERE " + where(foo), tuple[int, str])
```

## A literal dict lookup

```python
from typing import Literal

from db import fetch_all

ORDER = {"asc": "ASC", "desc": "DESC"}


def find(direction: Literal["asc", "desc"]) -> list[tuple[int]]:
    # error:
    return fetch_all("SELECT id FROM foos ORDER BY id " + ORDER[direction], tuple[int])
```

## Too many variants is reported, not skipped

The default `max-query-variants` is 64.

```python
from db import fetch_all


def find(a: bool, b: bool, c: bool, d: bool, e: bool, f: bool, g: bool) -> list[tuple[int]]:
    query = "SELECT id FROM foos WHERE TRUE"
    if a:
        query += " AND n = 1"
    if b:
        query += " AND n = 2"
    if c:
        query += " AND n = 3"
    if d:
        query += " AND n = 4"
    if e:
        query += " AND n = 5"
    if f:
        query += " AND n = 6"
    if g:
        query += " AND n = 7"
    # error: [dynamic-query] "128 variants, more than max-query-variants (64)"
    return fetch_all(query, tuple[int])
```

## A branch with a value that is not static

One such branch leaves the whole query unchecked.

```python
from typing import LiteralString

from db import fetch_all


def build(foo: str) -> LiteralString: ...


def by_call(foo: str | None) -> list[tuple[int]]:
    cond: LiteralString
    if foo is None:
        cond = "foo IS NULL"
    else:
        cond = build(foo)
    # error: [dynamic-query]
    return fetch_all("SELECT id FROM foos WHERE " + cond, tuple[int])


def by_fstring(n: int | None) -> list[tuple[int]]:
    if n is None:
        cond = "n IS NULL"
    else:
        cond = f"n = {n}"
    # error: [dynamic-query]
    return fetch_all("SELECT id FROM foos WHERE " + cond, tuple[int])
```

## A loop stays dynamic

Not modelled, so sqlty does not guess.

```python
from typing import LiteralString

from db import fetch_all


def find(columns: list[str]) -> list[tuple[int]]:
    query: LiteralString = "SELECT id FROM foos"
    for _ in columns:
        query += " ORDER BY id"
    # error: [dynamic-query]
    return fetch_all(query, tuple[int])
```

## A name reassigned after the branch has one variant

```python
from typing import LiteralString

from db import fetch_all


def find(flag: bool) -> list[tuple[int]]:
    column: LiteralString = "foo" if flag else "id"
    column = "n"
    # error:
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## psycopg composition

`.format()` wraps a `str` in `sql.Literal`, a quoted value, so a fragment goes in `sql.SQL(...)`.

```python
from typing import Annotated, LiteralString

from psycopg import sql

from sqlty import SQL


def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...


def find(foo: str | None) -> list[tuple[int, str]]:
    var: LiteralString
    if foo is None:
        var = "foo IS NULL"
    else:
        var = "foo = %(foo)s"
    # error: [nullable-mismatch] "(variant: line 14)"
    return fetch_all(sql.SQL("SELECT id, foo FROM foos WHERE {}").format(sql.SQL(var)), tuple[int, str])
```
