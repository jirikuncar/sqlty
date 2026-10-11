# Row types

`sqlty rewrite` writes `SQL[tuple[...]]`. You can replace the tuple with a
named row type, and `sqlty check` validates it against the query columns.
Supported forms are `TypedDict`, `NamedTuple`, `@dataclass` and Pydantic
`BaseModel`.

## Named rows

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
class User:
    id: int
    name: str

class UserModel(BaseModel):
    id: int
    name: str

a = SQL[UserDict]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
b = SQL[UserTuple]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
c = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
d = SQL[UserModel]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

`# error:` lines sit above the line `sqlty check` reports on, warnings
included; a bare `# error:` means it reports nothing.

!!! warning "Do not strip files with row types"
    `sqlty strip` turns `SQL[User](...)` back into a plain string, and the
    next `rewrite` writes `SQL[tuple[...]]`. Edit such files in place; see
    [Editing queries](writing-queries.md#editing-queries).

## What is checked

Fields are matched to columns by position, in this order:

1. Field count
   ([`column-count-mismatch`](../reference/rules.md#column-count-mismatch),
   error).
2. Field types
   ([`column-type-mismatch`](../reference/rules.md#column-type-mismatch),
   error).
3. Field names against column names or aliases
   ([`column-name-mismatch`](../reference/rules.md#column-name-mismatch),
   warning).

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

class Account(TypedDict):
    user_id: int
    name: str

class Short(TypedDict):
    id: int

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
a = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-name-mismatch] "Account.user_id doesn't match query column 'id'"
b = SQL[Account]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-count-mismatch] "Short has 1 fields but query returns 2 columns"
c = SQL[Short]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

A field passes the type check when it can hold the column's values: `Any`, a
union containing `Any` or the column type, and `float` for an `int` column
are all accepted, but `bool` for an `int` column is not. A `NewType` is
accepted wherever its supertype is, and a parametrised generic such as
`list[str]` or `dict[str, int]` for a column that infers the bare `list` or
`dict`.

Annotations are resolved with Python's typing machinery when the module can
be imported, so `from datetime import datetime` matches a `TIMESTAMP` column
just like `datetime.datetime` does:

```python
from datetime import datetime
from typing import NamedTuple
from uuid import UUID
from sqlty import SQL

class Event(NamedTuple):
    id: int
    created_at: datetime
    token: UUID

q = SQL[Event]("--sql\nSELECT id::INTEGER, created_at::TIMESTAMP, token::UUID FROM events")
# error:
```

## TypedDict qualifiers

`ReadOnly`, `Required` and `NotRequired` are looked through before types are
compared. An open TypedDict is still matched by position, so a `NotRequired`
key needs a column:

```python
from typing import NotRequired, ReadOnly, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: ReadOnly[int]
    name: NotRequired[str]

ok = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-count-mismatch] "User has 2 fields but query returns 1 columns"
short = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
```

## Closed TypedDicts

A closed TypedDict ([PEP 728](https://peps.python.org/pep-0728/):
`closed=True`, `extra_items=Never`, or a subclass of either) is matched by
key instead of position. `NotRequired` keys may be missing and column order
does not matter. Undeclared columns and missing required keys are
[`typeddict-key-mismatch`](../reference/rules.md#typeddict-key-mismatch)
errors:

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str
    email: NotRequired[str]

reordered = SQL[User]("--sql\nSELECT name::TEXT, id::INTEGER FROM users")
# error: [typeddict-key-mismatch] "User has no key for query column 'phone'"
extra = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, phone::TEXT FROM users")
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
missing = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

With `extra_items=T`, extra columns are allowed and must have type `T`:

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=ReadOnly[int]):
    day: str

counts = SQL[Metrics]("--sql\nSELECT day::TEXT, views::INTEGER, clicks::INTEGER FROM stats")
# error: [column-type-mismatch] "Metrics.ratio is extra_items int but query column 'ratio' infers float"
ratio = SQL[Metrics]("--sql\nSELECT day::TEXT, ratio::FLOAT FROM stats")
```

## TypedDict inheritance

Subclasses must respect a base's openness (PEP 728). Violations are
[`typeddict-inheritance`](../reference/rules.md#typeddict-inheritance) errors
on the subclass:

- A closed base admits no new keys, no `extra_items` and no `closed=False`.
- A base with mutable `extra_items` must be kept exactly: no `closed=True`,
  `extra_items=Never`, `ReadOnly` or other type. New keys must be
  `NotRequired` and of that type.
- A base with `ReadOnly` `extra_items` may be narrowed or closed; new keys
  must be assignable to its type.
- Keys brought in by a sibling base count as new.

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
class User(Base):
    name: str

class Counters(TypedDict, extra_items=int):
    id: int

class Page(Counters):
    views: NotRequired[int]

# error: [typeddict-inheritance] "Bad.clicks must be NotRequired: base Counters has extra_items int"
class Bad(Counters):
    clicks: int

a = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
b = SQL[Page]("--sql\nSELECT id::INTEGER, views::INTEGER FROM pages")
c = SQL[Bad]("--sql\nSELECT id::INTEGER, clicks::INTEGER FROM pages")
```

A rejected override keeps the base's openness, so the subclass's keys are
still matched against the query. Only bases defined in the same file and
spelled as bare names are checked, and types are compared only when the
answer is certain (plain classes and unions of them, with `int` to `float`
to `complex` promotion).

## The row type at runtime

`SQL[T](...)` records `T` on the instance.
[`row_form`](../reference/python-api.md#row_formquery-default) returns it:

```python
from sqlty import SQL, row_form

query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
row_form(query)  # tuple[int, str]
row_form("--sql\nSELECT 1::INTEGER", None)  # None
```

Only an instance built as `SQL[T](...)` carries `T`. A plain string,
`SQL("...")`, an annotation-only `q: SQL[T] = "..."` and the result of string
operations such as `query + " LIMIT 1"` do not: `row_form` raises
`TypeError` for them unless you pass a default. The
[psycopg helpers](psycopg.md) use `row_form` to pick a row factory.

## Limitations

- The row type must be defined in the same file as the `SQL[T](...)` call.
  Imported row types are skipped silently.
- A column with neither a cast nor a schema type is typed `str` by
  `rewrite`, but `check` does not compare it with the field type.
- Validation is static. Nothing checks rows at runtime; use Pydantic if you
  need that.

Example project:
[`examples/04_check_and_types/`](https://github.com/jirikuncar/sqlty/tree/main/examples/04_check_and_types).
