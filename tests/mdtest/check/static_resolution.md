# Static resolution of row types

`check` resolves row type fields from source. These cases pin what it reports
for annotations that go through module-level names, string annotations,
standard-library classes and other modules, so the resolver can change without
changing what users see.

```toml
[tool.sqlty]
dialect = "postgres"
```

## Local aliases

### Assignment

```python
from typing import TypedDict
from sqlty import SQL

Id = int

class Row(TypedDict):
    id: Id

# error: [column-type-mismatch] "Row.id is Id but query column 'id' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id")
```

### `type` statement

```python
from typing import TypedDict
from sqlty import SQL

type Id = int

class Row(TypedDict):
    id: Id

# error: [column-type-mismatch] "Row.id is Id but query column 'id' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id")
```

### `TypeAlias` annotation

```python
from typing import TypeAlias, TypedDict
from sqlty import SQL

Id: TypeAlias = int

class Row(TypedDict):
    id: Id

# error: [column-type-mismatch] "Row.id is Id but query column 'id' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id")
```

### Alias of an alias

```python
from typing import TypedDict
from sqlty import SQL

Id = int
MaybeId = Id | None

class Row(TypedDict):
    id: MaybeId

# error: [column-type-mismatch] "Row.id is MaybeId but query column 'id' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id")
```

### `NewType` of an alias

```python
from typing import NewType, TypedDict
from sqlty import SQL

Id = int
UserId = NewType("UserId", Id)

class Row(TypedDict):
    id: UserId

# error: [column-type-mismatch] "Row.id is UserId but query column 'id' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id")
```

### Alias defined after the class

```python
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    id: "Id"

Id = int

# error: [column-type-mismatch] "Row.id is 'Id' but query column 'id' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id")
```

### `Literal` alias

```python
from typing import Literal, TypedDict
from sqlty import SQL

Status = Literal["new", "old"]

class Row(TypedDict):
    status: Status

# error:
q = SQL[Row]("--sql\nSELECT 'gone'::TEXT AS status")
```

## String annotations

### Builtin

```python
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    id: "int"

# error: [column-type-mismatch] "Row.id is 'int' but query column 'id' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id")
```

### `from __future__ import annotations`

```python
from __future__ import annotations

from typing import TypedDict
from sqlty import SQL

Id = int

class Row(TypedDict):
    id: Id

# error: [column-type-mismatch] "Row.id is Id but query column 'id' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id")
```

## Standard library classes

### Module attribute

```python
import ipaddress
import pathlib
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    address: ipaddress.IPv4Address
    path: pathlib.Path

# error: [column-type-mismatch] "Row.address is ipaddress.IPv4Address but query column 'address' infers int"
# error: [column-type-mismatch] "Row.path is pathlib.Path but query column 'path' infers int"
q = SQL[Row]("--sql\nSELECT 1::INTEGER AS address, 1::INTEGER AS path")
```

### Imported name

```python
from fractions import Fraction
from pathlib import Path
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    path: Path
    ratio: Fraction

# error: [column-type-mismatch] "Row.path is Path but query column 'path' infers int"
# error: [column-type-mismatch] "Row.ratio is Fraction but query column 'ratio' infers str"
q = SQL[Row]("--sql\nSELECT 1::INTEGER AS path, 'x'::TEXT AS ratio")
```

### Derived from a column type

A class keeps the column types it derives from: a `StrEnum` field wants an enum
member, not any `str`, and an `OrderedDict` takes what a `dict` takes.

```python
import collections
import enum
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    color: enum.StrEnum
    level: enum.IntEnum
    data: collections.OrderedDict
    other: collections.OrderedDict

# error: [column-type-mismatch] "Row.color is enum.StrEnum but query column 'color' infers str"
# error: [column-type-mismatch] "Row.level is enum.IntEnum but query column 'level' infers int"
# error: [column-type-mismatch] "Row.other is collections.OrderedDict but query column 'other' infers int"
q = SQL[Row]("--sql\nSELECT 'x'::TEXT AS color, 1::INTEGER AS level, '{}'::JSONB AS data, 1::INTEGER AS other")
```

## User classes

### Subclass of a builtin

```python
from typing import TypedDict
from sqlty import SQL

class Email(str):
    pass

class Row(TypedDict):
    email: Email

# error:
q = SQL[Row]("--sql\nSELECT 1::INTEGER AS email")
```

### Enum

```python
import enum
from typing import TypedDict
from sqlty import SQL

class Color(enum.StrEnum):
    RED = "red"

class Row(TypedDict):
    color: Color

# error:
q = SQL[Row]("--sql\nSELECT 1::INTEGER AS color")
```

## Other modules

### Alias chain

`ids.py`:

```python
from typing import NewType, TypeAlias

Plain = int
Alias: TypeAlias = Plain
type Name = str
UserId = NewType("UserId", Alias)
```

```python
from typing import TypedDict
from sqlty import SQL
from ids import Alias, Name, UserId

class Row(TypedDict):
    id: UserId
    other: Alias
    name: Name

# error: [column-type-mismatch] "Row.id is UserId but query column 'id' infers str"
# error: [column-type-mismatch] "Row.name is Name but query column 'name' infers int"
# error: [column-type-mismatch] "Row.other is Alias but query column 'other' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id, 'b'::TEXT AS other, 1::INTEGER AS name")
```

### Re-export from a package

`pkg/__init__.py`:

```python
from .ids import UserId as UserId
from . import ids as ids
```

`pkg/ids.py`:

```python
from typing import NewType

UserId = NewType("UserId", int)
```

```python
from typing import TypedDict
from sqlty import SQL
import pkg
from pkg import UserId

class Row(TypedDict):
    id: UserId
    other: pkg.ids.UserId

# error: [column-type-mismatch] "Row.id is UserId but query column 'id' infers str"
# error: [column-type-mismatch] "Row.other is pkg.ids.UserId but query column 'other' infers str"
q = SQL[Row]("--sql\nSELECT 'a'::TEXT AS id, 'b'::TEXT AS other")
```

### Module that is not available

```python
from typing import TypedDict
from sqlty import SQL
from not_installed import Thing

class Row(TypedDict):
    thing: Thing

# error:
q = SQL[Row]("--sql\nSELECT 1::INTEGER AS thing")
```
