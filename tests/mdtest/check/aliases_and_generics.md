# Row types through aliases and generics

A row type named through a module-level alias is checked as what it names, and
a generic class with its type arguments applied.

## Aliases of tuples

`type Row = ...`, `Row: TypeAlias = ...` and `Row = tuple[...]` all count.

```python
from typing import TypeAlias

from sqlty import SQL

type Pair = tuple[int, str]
Single: TypeAlias = tuple[int]
Plain = tuple[int, str]

# error: [column-type-mismatch] "Pair item 1 is str but query column 'int4' infers int"
a = SQL[Pair]("--sql\nSELECT 1::INTEGER, 2::INTEGER")
b = SQL[Pair]("--sql\nSELECT 1::INTEGER, 'x'::TEXT")
# error: [column-count-mismatch] "Single has 1 items but query returns 2 columns"
c = SQL[Single]("--sql\nSELECT 1::INTEGER, 2::INTEGER")
# error: [column-type-mismatch] "Plain item 0 is int but query column 'text' infers str"
d = SQL[Plain]("--sql\nSELECT 'x'::TEXT, 'y'::TEXT")
```

## Aliases of row classes

```python
from typing import TypedDict

from sqlty import SQL

class User(TypedDict):
    id: int

Row = User
type Other = Row

# error: [column-type-mismatch] "User.id is int but query column 'id' infers str"
q = SQL[Other]("--sql\nSELECT 'x'::TEXT AS id")
```

## Generic classes

The parameters of `class Row[T]` or of a `Generic[T]` base take the arguments.

```python
from dataclasses import dataclass
from typing import Generic, TypedDict, TypeVar

from sqlty import SQL

T = TypeVar("T")

class Row[V](TypedDict):
    id: V
    tags: list[V]

class Old(TypedDict, Generic[T]):
    id: T

@dataclass
class Box[V]:
    value: V | None

# error: [column-type-mismatch] "Row[int].id is int but query column 'id' infers str"
a = SQL[Row[int]]("--sql\nSELECT 'x'::TEXT AS id, ARRAY[1]::INTEGER[] AS tags")
b = SQL[Row[str]]("--sql\nSELECT 'x'::TEXT AS id, ARRAY['a']::TEXT[] AS tags")
# error: [column-type-mismatch] "Old[int].id is int but query column 'id' infers str"
c = SQL[Old[int]]("--sql\nSELECT 'x'::TEXT AS id")
# error: [column-type-mismatch] "Box[int].value is int but query column 'value' infers str"
d = SQL[Box[int]]("--sql\nSELECT 'x'::TEXT AS value")
```
