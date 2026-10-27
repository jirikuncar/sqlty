# Query variant limits

The bounds of the walk that reads query variants. Past one, the query is reported, never
checked on part of its variants.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
dynamic-query = "warn"
unchecked-query = "warn"
```

`schema.sql`:

```sql
CREATE TABLE foos (id INTEGER PRIMARY KEY, foo TEXT, n INTEGER NOT NULL);
```

`db.py`:

```python
from typing import Annotated, LiteralString

from psycopg import sql

from sqlty import SQL


def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...
```

## Preview off leaves the query dynamic

```toml
[tool.sqlty]
schema = "schema.sql"
preview = false

[tool.sqlty.rules]
dynamic-query = "warn"
```

```python
from db import fetch_all


def find(flag: bool) -> list[tuple[str]]:
    column = "id" if flag else "n"
    # error: [dynamic-query] "query is not a string literal, so it is not checked"
    return fetch_all("SELECT " + column + " FROM foos", tuple[str])
```

## A lower max-query-variants

```toml
[tool.sqlty]
schema = "schema.sql"
max-query-variants = 1

[tool.sqlty.rules]
dynamic-query = "warn"
```

```python
from db import fetch_all


def find(flag: bool) -> list[tuple[int]]:
    column = "id" if flag else "n"
    # error: [dynamic-query] "query has 2 variants, more than max-query-variants (1), so it is not checked"
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## More paths than the walk follows

The walk stops at 4096 paths, whatever `max-query-variants` is.

```python
from db import fetch_all


def find(f0: bool, f1: bool, f2: bool, f3: bool, f4: bool, f5: bool, f6: bool, f7: bool, f8: bool, f9: bool, f10: bool, f11: bool, f12: bool) -> list[tuple[int]]:
    query = "SELECT id FROM foos WHERE TRUE"
    if f0:
        query += " AND n = 0"
    if f1:
        query += " AND n = 1"
    if f2:
        query += " AND n = 2"
    if f3:
        query += " AND n = 3"
    if f4:
        query += " AND n = 4"
    if f5:
        query += " AND n = 5"
    if f6:
        query += " AND n = 6"
    if f7:
        query += " AND n = 7"
    if f8:
        query += " AND n = 8"
    if f9:
        query += " AND n = 9"
    if f10:
        query += " AND n = 10"
    if f11:
        query += " AND n = 11"
    if f12:
        query += " AND n = 12"
    # error: [dynamic-query] "query has over 4096 variants, more than max-query-variants (64)"
    return fetch_all(query, tuple[int])
```

## Too many variants of a composition is unchecked-query

```toml
[tool.sqlty]
schema = "schema.sql"
max-query-variants = 1

[tool.sqlty.rules]
unchecked-query = "warn"
```

```python
from psycopg import sql

from db import fetch_all


def find(flag: bool) -> list[tuple[int]]:
    column = "id" if flag else "n"
    # error: [unchecked-query] "query has 2 variants, more than max-query-variants (1)"
    return fetch_all(sql.SQL("SELECT {} FROM foos").format(sql.Identifier(column)), tuple[int])
```

## An expression nested deeper than 64 is dynamic

```python
from db import fetch_all


def find() -> list[tuple[int]]:
    query = "SELECT id FROM foos" + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " " + " "
    # error: [dynamic-query]
    return fetch_all(query, tuple[int])
```

## Functions are followed 8 calls deep

```python
from typing import LiteralString

from db import fetch_all


def part0() -> LiteralString:
    return "id"


def part1() -> LiteralString:
    return part0()


def part2() -> LiteralString:
    return part1()


def part3() -> LiteralString:
    return part2()


def part4() -> LiteralString:
    return part3()


def part5() -> LiteralString:
    return part4()


def part6() -> LiteralString:
    return part5()


def part7() -> LiteralString:
    return part6()


def part8() -> LiteralString:
    return part7()


def part9() -> LiteralString:
    return part8()


def find(flag: bool) -> list[tuple[int]]:
    column = "n" if flag else part8()
    # error: [dynamic-query]
    rows = fetch_all("SELECT " + column + " FROM foos", tuple[int])
    other = "foo" if flag else part7()
    # error: [column-type-mismatch] "(variant: line 49)"
    return fetch_all("SELECT " + other + " FROM foos", tuple[int])
```

## A name read before any binding drops the path

```python
from db import fetch_all


def find(flag: bool) -> list[tuple[str]]:
    if flag:
        column = "foo"
    # error: [nullable-mismatch]
    return fetch_all("SELECT " + column + " FROM foos", tuple[str])
```

## A name bound outside the function is dynamic

```python
from db import fetch_all


def outer(flag: bool) -> None:
    column = "id" if flag else "n"

    def find() -> list[tuple[int]]:
        # error: [dynamic-query]
        return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## A name bound in a with block is dynamic

A context manager can swallow an exception halfway through its block.

```python
import contextlib

from db import fetch_all


def find(flag: bool) -> list[tuple[int]]:
    column = "id"
    with contextlib.suppress(KeyError):
        column = {"a": "n"}["b"]
    # error: [dynamic-query]
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```
