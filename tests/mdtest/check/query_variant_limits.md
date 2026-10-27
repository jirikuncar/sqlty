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

## A name bound other than by a plain assignment is dynamic

```python
from typing import Literal

from db import fetch_all

ORDER = {"asc": "ASC", "desc": "DESC"}


def unpacked(pair: tuple[str, str]) -> list[tuple[int]]:
    column, *rest = pair
    # error: [dynamic-query]
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])


def repeated() -> list[tuple[int]]:
    column = "id"
    column *= 2
    # error: [dynamic-query]
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])


def caught() -> list[tuple[int]]:
    column = "id"
    try:
        pass
    except ValueError as column:
        pass
    # error: [dynamic-query]
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])


def captured(value: object) -> list[tuple[int]]:
    column = "id"
    match value:
        case str(column):
            pass
    # error: [dynamic-query]
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])


def declared() -> list[tuple[int]]:
    global ORDER
    # error: [dynamic-query]
    return fetch_all("SELECT id FROM foos ORDER BY id " + ORDER["asc"], tuple[int])


def walrus(columns: list[str]) -> list[tuple[int]]:
    if (column := columns[0]) is None:
        column = "id"
    # error: [dynamic-query]
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])


def nested() -> list[tuple[int]]:
    def column() -> str:
        return "id"

    # error: [dynamic-query]
    return fetch_all("SELECT " + column() + " FROM foos", tuple[int])


def not_a_string(direction: Literal["asc", "desc"]) -> list[tuple[int]]:
    # error: [dynamic-query]
    return fetch_all(ORDER, tuple[int])


def mixed() -> list[tuple[int]]:
    count = 1
    # error: [dynamic-query]
    return fetch_all("SELECT id FROM foos LIMIT " + count, tuple[int])
```

## Unpacked literals are followed

```python
from db import fetch_all


def find(flag: bool) -> list[tuple[int]]:
    if flag:
        column, table = "n", "foos"
    else:
        [column, table] = ["foo", "foos"]
    # error: [column-type-mismatch] "(variant: line 8)"
    return fetch_all("SELECT " + column + " FROM " + table, tuple[int])
```

## A call no path reaches is dynamic

```python
from db import fetch_all


def find(flag: bool) -> list[tuple[int]]:
    column = "id" if flag else "n"
    raise NotImplementedError
    # error: [dynamic-query]
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])
```

## A call in a class body is dynamic

```python
from db import fetch_all

COLUMN = "id"


class Repository:
    # error: [dynamic-query]
    rows = fetch_all("SELECT " + COLUMN + " FROM foos", tuple[int])
```

## A call inside a loop, try or with block reads the paths before it

```python
import contextlib

from db import fetch_all


def find(flag: bool, ids: list[int]) -> None:
    column = "foo" if flag else "id"
    for _ in ids:
        # error: [column-type-mismatch] "(variant: line 7)"
        fetch_all("SELECT " + column + " FROM foos", tuple[int])
    try:
        pass
    except ValueError:
        # error: [column-type-mismatch] "(variant: line 7)"
        fetch_all("SELECT " + column + " FROM foos", tuple[int])
    with contextlib.suppress(ValueError):
        # error: [column-type-mismatch] "(variant: line 7)"
        fetch_all("SELECT " + column + " FROM foos", tuple[int])
    if flag:
        pass
    # The `else` of `if flag` reads only the variant of `flag` false.
    elif fetch_all("SELECT " + column + " FROM foos", tuple[str]):  # error: [column-type-mismatch] "'id' infers int"
        pass
```

## Functions with arguments are followed

```python
from typing import Literal, LiteralString

from db import fetch_all


def column(name: Literal["id", "n"], quoted: bool = False) -> LiteralString:
    return name


def nullable(name: str) -> LiteralString:
    if name:
        return "foo"
    return "id"


def find() -> list[tuple[int]]:
    # error: [column-type-mismatch] "(variant: line 12)"
    rows = fetch_all("SELECT " + column("id") + ", " + nullable("x") + " FROM foos", tuple[int, int])
    return rows + fetch_all("SELECT " + column(name="n") + " FROM foos", tuple[int])
```

## More static values

```python
from typing import Literal, LiteralString

from db import fetch_all

COLUMNS = {"id": "id", "n": "n"}


def suffix(column: str = "id") -> LiteralString:
    return " ORDER BY " + column


def order(flag: bool) -> list[tuple[int]]:
    if flag:
        pass
    match flag:
        case True:
            pass
    return fetch_all("SELECT id FROM foos" + suffix(), tuple[int]) + fetch_all(
        "SELECT id FROM foos" + suffix(column="n"), tuple[int]
    )


def quoted(column: "Literal['id', 'foo']") -> list[tuple[int]]:
    # error: [column-type-mismatch] "(variant: line 23)"
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])


def matched(column: Literal["id", "foo", "n"]) -> list[tuple[int]]:
    match column:
        case "foo" | "x":
            return []
        case str() if not column:
            pass
        case "id":
            query = "SELECT id FROM foos"
    # `foo` returns before the call: `id` and `n` reach it.
    return fetch_all("SELECT " + column + " FROM foos", tuple[int])


def inside(column: Literal["id", "foo"]) -> list[tuple[int]]:
    match column:
        case "foo":
            pass
        case _:
            # The `case _` reads only the variant of `column` other than `"foo"`.
            return fetch_all("SELECT " + column + " FROM foos", tuple[int])
    return []


def select(column: str) -> LiteralString:
    return "SELECT " + column + " FROM foos"


def forwarded(flag: bool) -> list[tuple[int]]:
    # error: [column-type-mismatch] "'foo' infers str (variant: line"
    return fetch_all(select("foo" if flag else "id"), tuple[int])
```

## A `Literal` parameter in a psycopg composition

```python
from typing import Literal

from psycopg import sql

from db import fetch_all


def find(column: Literal["id", "foo"]) -> list[tuple[int]]:
    # error: [column-type-mismatch] "'foo' infers str (variant: line 8)"
    return fetch_all(sql.SQL("SELECT {} FROM foos").format(sql.Identifier(column)), tuple[int])
```

## More dynamic values

```python
from typing import LiteralString

from db import fetch_all

ORDER = {"asc": "ASC", "desc": "DESC"}


def nothing(flag: bool) -> LiteralString:
    if flag:
        return "id"
    return


def imported() -> list[tuple[int]]:
    from os import sep

    # error: [dynamic-query]
    return fetch_all("SELECT id FROM foos" + sep, tuple[int])


def bare(flag: bool) -> list[tuple[int]]:
    # error: [dynamic-query]
    return fetch_all("SELECT " + nothing(flag) + " FROM foos", tuple[int])


def added() -> list[tuple[int]]:
    # error: [dynamic-query]
    return fetch_all("SELECT id FROM foos" + ORDER + "x", tuple[int])


def indexed() -> list[tuple[int]]:
    # error: [dynamic-query]
    return fetch_all("SELECT id FROM foos" + "abc"[0], tuple[int])


def keyed(flag: bool) -> list[tuple[int]]:
    # error: [dynamic-query]
    return fetch_all("SELECT id FROM foos" + {"a" if flag else "b": "c"}["a"], tuple[int])
```
