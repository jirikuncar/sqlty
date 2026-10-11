# Parametrised Generic and `NewType` Fields

Array columns infer a bare `list`, which a `list[str]` or `list[Any]` field
accepts. A `NewType` field accepts the columns its supertype accepts. This
holds whether field types are resolved at runtime or read from source (when
an import is unavailable).

```toml
[tool.sqlty]
dialect = "postgres"
```

## `list[...]` and `NewType` fields

```python
from typing import Any, NewType, TypedDict
from sqlty import SQL

UserId = NewType("UserId", int)

class RowAny(TypedDict):
    id: int
    tags: list[Any]

class RowStr(TypedDict):
    id: UserId
    tags: list[str]

a = SQL[RowAny]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
b = SQL[RowStr]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# error:
```

## `list[...]` and `NewType` fields without runtime resolution

```python
import not_installed
from typing import NewType, TypedDict
from sqlty import SQL

UserId = NewType("UserId", int)

class Row(TypedDict):
    id: UserId
    tags: list[str]

q = SQL[Row]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# error:
```

## Mismatched `list[...]` and `NewType` fields

```python
from typing import NewType, TypedDict
from sqlty import SQL

Name = NewType("Name", str)

class Row(TypedDict):
    id: Name
    tags: dict[str, str]

# error: [column-type-mismatch] "Row.id is Name but query column 'id' infers int"
# error: [column-type-mismatch] "Row.tags is dict[str, str] but query column 'tags' infers list"
q = SQL[Row]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
```

## Parametrised tuple items are not stale

```python
from sqlty import SQL

q = SQL[tuple[int, list[str]]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# error:
```

## Parametrised tuple items are kept by `--fix`

```python
from sqlty import SQL

q = SQL[tuple[int, list[str]]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, list[str]]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
```

## Changed tuple items are still stale

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str, list[str]]] should be SQL[tuple[int, list]]"
q = SQL[tuple[str, list[str]]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, list]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
```

## Unions with a parametrised item are still stale

Only an item that is exactly `list[...]` refines an inferred `list`;
`list[str] | str` and `list[str] | None` are reported.

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int, list[str] | str]] should be SQL[tuple[int, list]]"
a = SQL[tuple[int, list[str] | str]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# error: [stale-type] "SQL[tuple[int, list[str] | None]] should be SQL[tuple[int, list]]"
b = SQL[tuple[int, list[str] | None]]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
```

## Optional and nested `list[...]` fields

```python
from typing import Optional, TypedDict
from sqlty import SQL

class Row(TypedDict):
    tags: Optional[list[str]]
    labels: list[str] | None
    grid: list[list[int]]

q = SQL[Row]("--sql\nSELECT tags::TEXT[], labels::TEXT[], grid::INTEGER[][] FROM users")
# error:
```

## `NewType` of a `NewType` and of `list[str]`

```python
from typing import NewType, TypedDict
from sqlty import SQL

UserId = NewType("UserId", int)
AdminId = NewType("AdminId", UserId)
Tags = NewType("Tags", list[str])

class Row(TypedDict):
    id: AdminId
    tags: Tags

class Bad(TypedDict):
    id: AdminId

q = SQL[Row]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# error: [column-type-mismatch] "Bad.id is AdminId but query column 'id' infers str"
r = SQL[Bad]("--sql\nSELECT id::TEXT FROM users")
```

## `NewType` of a `NewType` and of `list[str]` without runtime resolution

```python
import not_installed
from typing import NewType, TypedDict
from sqlty import SQL

UserId = NewType("UserId", int)
AdminId = NewType("AdminId", UserId)
Tags = NewType("Tags", list[str])

class Row(TypedDict):
    id: AdminId
    tags: Tags

class Bad(TypedDict):
    id: AdminId

q = SQL[Row]("--sql\nSELECT id::INTEGER, tags::TEXT[] FROM users")
# error: [column-type-mismatch] "Bad.id is AdminId but query column 'id' infers str"
r = SQL[Bad]("--sql\nSELECT id::TEXT FROM users")
```

## Element types are not checked

Array columns infer a bare `list`, so the element type of a `list[...]` field
is not compared: a `list[int]` field on a `TEXT[]` column is accepted.

```python
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    tags: list[int]

q = SQL[Row]("--sql\nSELECT tags::TEXT[] FROM users")
# error:
```
