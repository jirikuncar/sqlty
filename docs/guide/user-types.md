# User-Defined Row Types

`sqlty rewrite` produces `SQL[tuple[...]]`. Replace the tuple with a named
row type and `sqlty check` validates it against the query: field count,
field types and field names. Runnable project:
[`examples/04_check_and_types/`](../../examples/04_check_and_types/).

## Supported forms

`TypedDict`, `NamedTuple`, `@dataclass` and Pydantic `BaseModel` are all
checked structurally:

```python
from dataclasses import dataclass
from typing import NamedTuple, TypedDict

from pydantic import BaseModel

from sqlty import SQL


class UserDict(TypedDict):
    id: int
    name: str


class UserTuple(NamedTuple):
    id: int
    name: str


@dataclass
class UserData:
    id: int
    name: str


class UserModel(BaseModel):
    id: int
    name: str


as_dict = SQL[UserDict]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
as_tuple = SQL[UserTuple]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
as_data = SQL[UserData]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
as_model = SQL[UserModel]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## What gets checked

The field count is checked first, then each field's type and name in order:

```python
from typing import TypedDict

from sqlty import SQL


class Short(TypedDict):
    id: int


class Wrong(TypedDict):
    id: str
    name: str


class Renamed(TypedDict):
    user_id: int
    name: str


# error: [column-count-mismatch] "Short has 1 fields but query returns 2 columns"
a = SQL[Short]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-type-mismatch] "Wrong.id is str but query column 'id' infers int"
b = SQL[Wrong]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-name-mismatch] "Renamed.user_id doesn't match query column 'id'"
c = SQL[Renamed]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Import styles resolve

Field types are resolved with Python's typing machinery, so
`from datetime import datetime` matches a `TIMESTAMP` column:

```python
from datetime import date, datetime
from typing import TypedDict
from uuid import UUID

from sqlty import SQL


class Event(TypedDict):
    id: int
    day: date
    created_at: datetime
    token: UUID


q = SQL[Event]("--sql\nSELECT id::INTEGER, day::DATE, created_at::TIMESTAMP, token::UUID FROM events")
# error:
```

## Closed TypedDicts

`ReadOnly`, `Required` and `NotRequired` are looked through. Closed TypedDicts
([PEP 728](https://peps.python.org/pep-0728/): `closed=True`,
`extra_items=Never`, or a subclass of either) match columns by key:
`NotRequired` keys may be absent, and extra columns are allowed only through
`extra_items`, whose type they must match.

```python
from typing import NotRequired, ReadOnly, TypedDict

from sqlty import SQL


class Profile(TypedDict, closed=True):
    id: int
    bio: NotRequired[str]


class Metrics(TypedDict, extra_items=int):
    day: ReadOnly[str]


ok = SQL[Profile]("--sql\nSELECT id::INTEGER FROM profiles")
# error: [typeddict-key-mismatch] "Profile has no key for query column 'name'"
extra = SQL[Profile]("--sql\nSELECT id::INTEGER, name::TEXT FROM profiles")
# error: [column-type-mismatch] "Metrics.ratio is extra_items int but query column 'ratio' infers float"
ratio = SQL[Metrics]("--sql\nSELECT day::TEXT, views::INTEGER, ratio::FLOAT FROM stats")
```

## Severity

`column-name-mismatch` is a warning by default; the others are errors.
See the [rules](../rules.md) to change severities.

## Limitation: same file only

The row type must be defined in the file that uses it. Imported types are
skipped without a diagnostic:

```python
from models import User

from sqlty import SQL

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error:
```
