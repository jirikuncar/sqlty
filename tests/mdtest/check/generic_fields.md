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
```
