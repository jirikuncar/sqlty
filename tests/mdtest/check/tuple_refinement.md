# Tuple annotations: stale or narrow

A `SQL[tuple[...]]` annotation that is wider than the query's row is `stale-type`: it still
holds, but a fix can tighten it. One that a row can break is `narrow-type`.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER,
    name TEXT NOT NULL,
    tags TEXT[] NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('a', 'b')),
    sign INTEGER NOT NULL CHECK (sign IN (-1, 2)),
    flag BOOLEAN NOT NULL CHECK (flag IN (TRUE)),
    word TEXT NOT NULL CHECK (word IN ('Decimal', 'a"Decimal'))
);
```

## A wider annotation is stale

```python
import typing
from typing import Literal

# error: [stale-type] "SQL[tuple[str | None]] should be SQL[tuple[str]]"
a = SQL[tuple[str | None]]("--sql\nSELECT name FROM users")
# error: [stale-type] "SQL[tuple[object, typing.Any]] should be SQL[tuple[int | None, str]]"
b = SQL[tuple[object, typing.Any]]("--sql\nSELECT id, name FROM users")
c = SQL[tuple[str, int, bool]]("--sql\nSELECT kind, sign, flag FROM users")
```

## An annotation a row can break is narrow

```python
# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int | None]]"
a = SQL[tuple[str]]("--sql\nSELECT id FROM users")
# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[int | None, str]]"
b = SQL[tuple[int]]("--sql\nSELECT id, name FROM users")
# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[typing.Literal['a', 'b']]]"
c = SQL[tuple[int]]("--sql\nSELECT kind FROM users")
# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[list[str]]]"
d = SQL[tuple[int]]("--sql\nSELECT tags FROM users")
```

## A parametrised item is not stale

```python
q = SQL[tuple[int, list[str]]]("--sql\nSELECT 1::INTEGER, tags FROM users")
# error:
```

## Quoted text is not an imported name

`Decimal` inside a string is a value, not the class `from decimal import Decimal` binds.

```python
from decimal import Decimal
from typing import Literal

q = SQL[tuple[Decimal, Literal['Decimal', "a\"Decimal"]]]("--sql\nSELECT 1::NUMERIC, word FROM users")
# error:
```

## Quoted, optional and signed annotations

A string annotation is read as the type it spells, and a `Literal` is compared with the values of its column.

```python
from typing import Literal

# error: [stale-type] "SQL[tuple["str | None"]] should be SQL[tuple[str]]"
a = SQL[tuple["str | None"]]("--sql\nSELECT name FROM users")
c = SQL[tuple[int]]("--sql\nSELECT sign FROM users")
# error: [literal-mismatch] "SQL[tuple[Literal[-1]]] item 0 is Literal[-1] but its query column can be 2"
d = SQL[tuple[Literal[-1]]]("--sql\nSELECT sign FROM users")
e = SQL[tuple[Literal[-1, 2]]]("--sql\nSELECT sign FROM users")
# error: [literal-mismatch] "SQL[tuple[Literal["a\n"]]] item 0 is Literal["a\n"] but its query column can be 'a', 'b'"
f = SQL[tuple[Literal["a\n"]]]("--sql\nSELECT kind FROM users")
```
