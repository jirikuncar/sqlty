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

## Pydantic types

With `preview = true`, Pydantic's types come from a static table, so `check`
gives the same result whether pydantic is installed or not.

### Model fields take what Pydantic validates

A model validates its input: `AnyUrl` takes the `str` a `TEXT` column holds,
`ByteSize` an `int`. Each comment is the value the driver returns.

```python
import pydantic
from pydantic import AnyUrl, BaseModel, EmailStr, HttpUrl, Json
from sqlty import SQL

class Row(BaseModel):
    email: EmailStr  # 'a@b.co'
    name: pydantic.NameEmail  # 'A <a@b.co>'
    url: AnyUrl  # 'https://a.co'
    http: HttpUrl  # 'https://a.co'
    secret: pydantic.SecretStr  # 's3cret'
    count: Json[int]  # '1'
    past: pydantic.PastDatetime  # datetime(2000, 1, 1)
    aware: pydantic.AwareDatetime  # datetime(2000, 1, 1, tzinfo=UTC)
    due: pydantic.FutureDate  # date(2100, 1, 1)
    size: pydantic.ByteSize  # 1024
    address: pydantic.IPvAnyAddress  # IPv4Address('127.0.0.1')

# error:
q = SQL[Row]("""--sql
SELECT 'a@b.co'::TEXT AS email, 'A <a@b.co>'::TEXT AS name,
    'https://a.co'::TEXT AS url, 'https://a.co'::TEXT AS http, 's3cret'::TEXT AS secret,
    '1'::TEXT AS count, '2000-01-01'::TIMESTAMP AS past, '2000-01-01'::TIMESTAMPTZ AS aware,
    '2100-01-01'::DATE AS due, 1024::INTEGER AS size, '127.0.0.1'::INET AS address""")
```

### Real mismatches

Pydantic rejects each of these values.

```python
import pydantic
from pydantic import BaseModel, conint, constr, condecimal
from sqlty import SQL

class Row(BaseModel):
    email: pydantic.EmailStr  # 1
    url: pydantic.AnyUrl  # 1
    positive: pydantic.PositiveInt  # 'x'
    bounded: conint(gt=0)  # 'x'
    short: constr(max_length=3)  # 1
    price: condecimal(max_digits=5)  # '2000-01-01'
    id: pydantic.UUID4  # 1
    encoded: pydantic.Base64Str  # 1
    size: pydantic.ByteSize  # '2000-01-01'
    address: pydantic.IPvAnyAddress  # date(2000, 1, 1)

# error: [column-type-mismatch] "Row.address is pydantic.IPvAnyAddress but query column 'address' infers datetime.date"
# error: [column-type-mismatch] "Row.bounded is conint(gt=0) but query column 'bounded' infers str"
# error: [column-type-mismatch] "Row.email is pydantic.EmailStr but query column 'email' infers int"
# error: [column-type-mismatch] "Row.encoded is pydantic.Base64Str but query column 'encoded' infers int"
# error: [column-type-mismatch] "Row.id is pydantic.UUID4 but query column 'id' infers int"
# error: [column-type-mismatch] "Row.positive is pydantic.PositiveInt but query column 'positive' infers str"
# error: [column-type-mismatch] "Row.price is condecimal(max_digits=5) but query column 'price' infers datetime.date"
# error: [column-type-mismatch] "Row.short is constr(max_length=3) but query column 'short' infers int"
# error: [column-type-mismatch] "Row.size is pydantic.ByteSize but query column 'size' infers datetime.date"
# error: [column-type-mismatch] "Row.url is pydantic.AnyUrl but query column 'url' infers int"
q = SQL[Row]("""--sql
SELECT 1::INTEGER AS email, 1::INTEGER AS url, 'x'::TEXT AS positive, 'x'::TEXT AS bounded,
    1::INTEGER AS short, '2000-01-01'::DATE AS price, 1::INTEGER AS id, 1::INTEGER AS encoded,
    '2000-01-01'::DATE AS size, '2000-01-01'::DATE AS address""")
```

### `TypedDict` fields are what a type checker sees

A `TypedDict` holds what the driver returns, so it gets the type a type checker
reads from pydantic: `EmailStr` is a `str`, and `AnyUrl` a class no column holds.

```python
from typing import TypedDict
from pydantic import AnyUrl, EmailStr, PastDatetime
from sqlty import SQL

class Row(TypedDict):
    email: EmailStr
    past: PastDatetime
    url: AnyUrl

# error: [column-type-mismatch] "Row.url is AnyUrl but query column 'url' infers str"
q = SQL[Row]("--sql\nSELECT 'a@b.co'::TEXT AS email, now()::TIMESTAMP AS past, 'https://a.co'::TEXT AS url")
```

### Imported from the defining module

```python
from pydantic.networks import EmailStr
from pydantic.types import PositiveInt
from pydantic.dataclasses import dataclass
from sqlty import SQL

@dataclass
class Row:
    email: EmailStr
    count: PositiveInt

# error: [column-type-mismatch] "Row.count is PositiveInt but query column 'count' infers str"
q = SQL[Row]("--sql\nSELECT 'a@b.co'::TEXT AS email, 'x'::TEXT AS count")
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
