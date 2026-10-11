# `Any` and Union Field Types

A field annotated `Any`, or a union containing `Any` or the inferred column
type, accepts the column: `column-type-mismatch` checks that the column's
values fit the field, not that the annotations are spelled the same.
`int` columns also fit `float` fields, but a narrower field such as `bool`
does not fit an `int` column. This holds whether field types are resolved at
runtime or read from source (when an import is unavailable).

```toml
[sqlty]
dialect = "postgres"
```

## NamedTuple fields typed `Any` and `int | Any`

```python
from typing import Any, NamedTuple
from sqlty import SQL

class Row(NamedTuple):
    id: int | Any
    name: str

class Row2(NamedTuple):
    id: Any
    name: str

a = SQL[Row]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
b = SQL[Row2]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## `Any` fields without runtime resolution

```python
import not_installed
from typing import Any, NamedTuple, Optional, Union
from sqlty import SQL

class Row(NamedTuple):
    id: int | Any
    name: Any
    score: Optional[Union[str, Any]]

q = SQL[Row]("--sql\nSELECT id::INTEGER, name::TEXT, score::FLOAT FROM users")
# error:
```

## Union containing the column type

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class Row:
    id: int | str

q = SQL[Row]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

## `int` columns fit `float` fields

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class Row:
    ratio: float

q = SQL[Row]("--sql\nSELECT ratio::INTEGER FROM users")
# error:
```

## Qualified and aliased `Any` without runtime resolution

```python
import not_installed
import typing
import typing as t
from typing import Annotated, NamedTuple
from typing import Any as A
from sqlty import SQL

class Row(NamedTuple):
    id: typing.Any
    name: t.Any
    score: A
    views: Annotated[int, "count"]

q = SQL[Row]("--sql\nSELECT id::INTEGER, name::TEXT, score::FLOAT, views::INTEGER FROM users")
# error:
```

## Narrower fields mismatch

A `bool` field can't hold every `INTEGER` value.

```python
from typing import NamedTuple
from sqlty import SQL

class Row(NamedTuple):
    flag: bool

q = SQL[Row]("--sql\nSELECT flag::INTEGER FROM users")
# error: [column-type-mismatch] "Row.flag is bool but query column 'flag' infers int"
```

## Narrower fields mismatch without runtime resolution

```python
import not_installed
from typing import NamedTuple
from sqlty import SQL

class Row(NamedTuple):
    flag: bool

q = SQL[Row]("--sql\nSELECT flag::INTEGER FROM users")
# error: [column-type-mismatch] "Row.flag is bool but query column 'flag' infers int"
```

## Union without the column type still mismatches

```python
import not_installed
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    id: str | bytes

q = SQL[Row]("--sql\nSELECT id::INTEGER FROM users")
# error: [column-type-mismatch] "Row.id is str | bytes but query column 'id' infers int"
```

## `extra_items=Any` accepts any extra column

```python
from typing import Any, TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=Any):
    day: str

q = SQL[Metrics]("--sql\nSELECT day::TEXT, ratio::FLOAT, views::INTEGER FROM stats")
# error:
```

## Annotation fields typed `Any`

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "User"
fields = [["id", "int | Any"], ["name", "Any"]]
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error:
```

## Narrower annotation fields mismatch

```toml
[sqlty]
dialect = "postgres"
[sqlty.annotation]
type = "User"
fields = [["flag", "bool"]]
```

```sql
SELECT flag::INTEGER FROM users
```

```python
# error: [column-type-mismatch] "User.flag is bool but query column 'flag' infers int"
```
