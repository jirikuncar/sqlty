# Row types

`sqlty rewrite` writes `SQL[tuple[...]]`. You can replace the tuple with a
named row type, and `sqlty check` validates it against the query columns.
Supported forms are `TypedDict`, `NamedTuple`, `@dataclass`, Pydantic
`BaseModel` and `Protocol`. `TypedDict` and `NamedTuple` work in their functional forms too:
`TypedDict("User", {"id": int})` and `NamedTuple("User", [("id", int)])`.

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

`TypedDict`, `@dataclass` and Pydantic `BaseModel` fields are matched to
columns by name, as psycopg's `dict_row`, `class_row` and `kwargs_row` pass
them, so the order of fields and columns does not matter:

1. Field count
   ([`column-count-mismatch`](../reference/rules.md#column-count-mismatch),
   error), counting only required fields as the minimum.
2. Field types
   ([`column-type-mismatch`](../reference/rules.md#column-type-mismatch),
   error).
3. Columns without a field and required fields without a column
   ([`typeddict-key-mismatch`](../reference/rules.md#typeddict-key-mismatch),
   error).

A field with a default, and a `NotRequired` key, may be missing from the
query. A `ClassVar`, a dataclass's `KW_ONLY` marker and `field(init=False)`
fields, and a Pydantic model's private `_attributes` are not fields: the
constructor takes no argument for them. A column without a field is an error: a TypedDict and a dataclass
reject it, and a Pydantic model drops it by default (`extra="ignore"`), so it is
fetched for nothing. Only a model with `extra="allow"` keeps it.

```python
from dataclasses import dataclass
from typing import TypedDict

from pydantic import BaseModel
from sqlty import SQL

class User(TypedDict):
    name: str
    id: int

@dataclass
class Account:
    id: int
    name: str
    email: str | None = None

class Profile(BaseModel):
    id: int

class Short(TypedDict):
    id: int

class Typed(TypedDict):
    id: str
    name: str

reordered = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
optional = SQL[Account]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [typeddict-key-mismatch] "Profile has no field for query column 'name'"
dropped = SQL[Profile]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-count-mismatch] "Short has 1 fields but query returns 2 columns"
c = SQL[Short]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-type-mismatch] "Typed.id is str but query column 'id' infers int"
a = SQL[Typed]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

`NamedTuple` fields are matched by position, in this order:

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
from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    id: str
    name: str

class Account(NamedTuple):
    user_id: int
    name: str

class Short(NamedTuple):
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
`list[str]` or `dict[str, int]` for a column that infers `list` or `dict`
(element types are not compared). A JSON column infers `Any`, so any field
accepts it.

Annotations are resolved one field at a time with Python's typing machinery,
so `from datetime import datetime` matches a `TIMESTAMP` column just like
`datetime.datetime` does. Aliases from the same module (`UserName = str`,
`type UserName = str`) and `from __future__ import annotations` work too. A
field whose annotation cannot be resolved, such as one imported from a module
that is not installed, is unknown and never reported; the other fields are
still checked:

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

### Row types from other modules

Row types usually live in a models module. `SQL[User](...)` with `User`
imported from another module of the checked paths is checked like a local
one: `from pkg.models import User [as U]`, `import pkg.models as m`
(`SQL[m.User]`), relative imports and `__init__.py` re-exports all resolve.
The module is read from its source, never imported.

```text
# pkg/models.py
class User(TypedDict):
    id: str

# app.py
from pkg.models import User
q = SQL[User]("--sql\nSELECT 1::INTEGER AS id")  # column-type-mismatch
```

Run `sqlty check` on a path that holds both files, such as the project root.

### Types from other modules

`sqlty check` also resolves a field annotated with a name imported from
another module of the checked paths, such as the shared `NewType` ID types of
an application. The module is read from its source and never imported: its
module-level `X = NewType("X", T)`, `X = Literal[...]`, `type X = ...`,
`X: TypeAlias = ...` and plain `X = <type expression>` are followed through
any number of further imports, aliases and `__init__.py` re-exports, and
import cycles are cut. `from pkg.ids import UserId [as U]`,
`import pkg.ids as ids` (`ids.UserId`) and relative imports all work.

```text
# pkg/ids.py
UserId = NewType("UserId", int)

# app.py
class User(TypedDict):
    id: UserId   # accepted for an int column, reported for a text column
```

A module is parsed only when a row type names something from it, and only once
per run. A name from a module outside the checked paths, or one that is not a
type definition, stays unknown and is never reported.

`check` never runs the imports of your application: it executes only imports
of the standard library, `typing`, `typing_extensions`, `pydantic`,
`pydantic_core`, `annotated_types` and `sqlty`, plus the modules listed in
[`type-modules`](../reference/configuration.md#type-modules). Everything else
is resolved from source as above or stays unknown, so a module with import-time
side effects is not triggered by `sqlty check`. From Python, pass a
`HelperIndex` filled with the modules (`index.add("pkg.ids", source)`) and the
`module` name of the checked source as `check_source(..., helpers=index,
module=...)`.

## Protocols

A `Protocol` row type says which attributes a row has, not how it is built.
Each attribute is matched to the column of its name, and the query may return
more columns than the protocol lists. At runtime the
[psycopg helpers](psycopg.md) build such rows with `namedtuple_row`, which
gives every column as an attribute; `class_row` cannot build a Protocol.

```python
from typing import Protocol

import psycopg
from sqlty import SQL
from sqlty.drivers.psycopg import execute

class Named(Protocol):
    id: int
    name: str

query = SQL[Named]("--sql\nSELECT 1::INTEGER AS id, 'ada'::TEXT AS name, now() AS seen")

with psycopg.connect("dbname=app") as conn:
    row = execute(conn, query).fetchone()  # Named | None
    if row is not None:
        print(row.id, row.name)  # 1 ada
# error:
```

`sqlty check` reports a missing attribute and a type mismatch:

```python
from typing import Protocol

from sqlty import SQL

class Named(Protocol):
    id: int
    name: str

# error: [typeddict-key-mismatch] "Named.name is required but missing from query"
bad = SQL[Named]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS title")
# error: [column-type-mismatch] "Named.id is int but query column 'id' infers str"
wrong = SQL[Named]("--sql\nSELECT 'x'::TEXT AS id, 'ada'::TEXT AS name")
```

Only annotated attributes count; methods and properties are not read.

## Composite columns

A column of a PostgreSQL composite type (`CREATE TYPE point2 AS (x int, y int)`)
has no Python type of its own: psycopg returns it as text unless you register
the type, and `register_composite` then builds the class you give it. When a
field of your row type is annotated with a row type (`NamedTuple`, dataclass,
`TypedDict`, Pydantic model), `sqlty check` compares the composite's fields with
it, as it compares query columns with a row type. Composite fields can always
be NULL.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TYPE point2 AS (x INTEGER, y INTEGER);
CREATE TABLE shapes (id INTEGER PRIMARY KEY, p point2);
```

```python
from typing import NamedTuple, TypedDict

import psycopg
from psycopg.types.composite import CompositeInfo, register_composite
from sqlty import SQL
from sqlty.drivers.psycopg import execute

class Point(NamedTuple):
    x: int | None
    y: int | None

class Shape(TypedDict):
    id: int
    p: Point | None

# error: [unknown-type] "column `shapes.p` has type `POINT2`, which has no Python type"
query = SQL[Shape]("--sql\nSELECT id, p FROM shapes")

with psycopg.connect("dbname=app") as conn:
    register_composite(CompositeInfo.fetch(conn, "point2"), conn, Point)
    shapes = execute(conn, query).fetchall()  # list[Shape], p a Point
```

A field type that does not match is reported with the composite's name:
`Shape.p.y is str but composite point2 field 'y' infers int`.

## Pydantic aliases and extra columns

A Pydantic model validates a row by alias, so a column is matched to a field by
its `Field(alias="id")` or `Field(validation_alias="id")` string (also inside
`Annotated`). With `populate_by_name=True` either the alias or the field name
fills the field. A column without a field is an error unless the model sets
`ConfigDict(extra="allow")` (or `class Config: extra = "allow"`). Aliases
and the config are read from the class source, so Pydantic need not be
installed; a config that cannot be read there, such as `model_config = SHARED`,
may allow extra columns and leaves them unreported. A model nested in a JSON column
reads the object's keys the same way.

```python
from pydantic import BaseModel, ConfigDict, Field
from sqlty import SQL

class User(BaseModel):
    model_config = ConfigDict(extra="forbid")
    user_id: int = Field(alias="id")

ok = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error: [typeddict-key-mismatch] "User has no field for query column 'name'"
bad = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

`AliasChoices`, `AliasPath`, a non-literal alias and `alias_generator` make a
field's names unknown: it is not reported as missing, and columns without a field
are no longer reported.

## Pydantic lax mode

A Pydantic model or `pydantic.dataclasses.dataclass` converts values in its
default lax mode, so its fields also accept the columns Pydantic converts: a
`Decimal` or `float` column for `int`, an `int` or `float` column for
`Decimal`, an `int` column for `bool` and a `date` column for `datetime`.
Value-dependent conversions (a whole `Decimal`, 0 or 1 for `bool`) are
accepted because the schema cannot rule them out. Strict mode keeps reporting
them: `ConfigDict(strict=True)` (or `class Config: strict = True`),
`Field(strict=True)`, `Annotated[int, Strict()]` and the `StrictInt`,
`StrictFloat`, `StrictBool` and `StrictStr` types. TypedDict, NamedTuple and
dataclass rows are not converted.

```python
from pydantic import BaseModel, ConfigDict, Field
from sqlty import SQL

class Totals(BaseModel):
    total: int

class StrictTotals(BaseModel):
    model_config = ConfigDict(strict=True)
    total: int
    count: int = Field(strict=False)

# error: [nullable-mismatch] "Totals.total is int but query column 'total' can be NULL"
ok = SQL[Totals]("--sql\nSELECT sum(id::BIGINT) AS total FROM users")
# error: [column-type-mismatch] "StrictTotals.total is int but query column 'total' infers decimal.Decimal"
# error: [nullable-mismatch] "StrictTotals.count is int but query column 'count' can be NULL"
bad = SQL[StrictTotals]("--sql\nSELECT sum(id::BIGINT) AS total, sum(id::BIGINT) AS count FROM users")
```

## Defaults in `Annotated`

A field whose `Annotated` metadata holds `Field(default=...)` or
`Field(default_factory=...)` (Pydantic, or a dataclass `field(...)` with a
default) is optional for every row type, TypedDict included, as is common when
rows are validated by `TypeAdapter(Row)` after psycopg builds them. A bare
`Field()` or `Field(...)` stays required.

```python
from typing import Annotated, TypedDict
from pydantic import Field
from sqlty import SQL

class User(TypedDict):
    id: int
    tags: Annotated[list[str], Field(default_factory=list)]

# error:
query = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
```

## TypedDict qualifiers

`ReadOnly`, `Required` and `NotRequired` are looked through before types are
compared. A `NotRequired` key may be missing from the query:

```python
from typing import NotRequired, ReadOnly, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: ReadOnly[int]
    name: NotRequired[str]

ok = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
short = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

## Closed TypedDicts

Open and closed TypedDicts ([PEP 728](https://peps.python.org/pep-0728/):
`closed=True`, `extra_items=Never`, or a subclass of either) are both matched
by key; `closed=True` states the intent, and `extra_items=T` (below) is what
admits extra columns. Undeclared columns and missing required keys are
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

## Aliases and generic row types

A row type named through a module-level alias (`type Row = ...`, `Row: TypeAlias = ...`,
`Row = tuple[...]` or `Row = OtherRow`) is checked as what it names, and a generic class
with its type arguments applied, so one class can serve several queries:

```python
from typing import TypedDict

from sqlty import SQL

class Page[T](TypedDict):
    total: int
    items: list[T]

type Pair = tuple[int, str]

ids = SQL[Page[int]]("--sql\nSELECT count(*) AS total, coalesce(array_agg(id), '{}')::INTEGER[] AS items FROM users")
# error: [column-type-mismatch] "Pair item 1 is str but query column 'id' infers int"
pair = SQL[Pair]("--sql\nSELECT id::INTEGER, id::INTEGER FROM users")
```

A generic alias (`type Rows[T] = list[T]`) is not resolved.

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

## Nullable columns

A column that can be NULL infers `T | None` (see
[Nullability](schema-inference.md#nullability)). A field that is not optional is
[`nullable-mismatch`](../reference/rules.md#nullable-mismatch), an error by default. A field
annotated `T | None` is accepted for a column that cannot be NULL: only
[`redundant-optional`](../reference/rules.md#redundant-optional), off by default, reports
that.

## Literal columns

A column with known values (an enum, a `CHECK (col IN (...))`, a `CASE` over constants; see
[Literal values](schema-inference.md#literal-values)) infers a `typing.Literal`. A field
annotated `str` or `int` takes it. A field annotated with a `Literal` must list every value:
a missing one is [`literal-mismatch`](../reference/rules.md#literal-mismatch), an error by
default. Values the `Literal` lists that the column never takes are
[`redundant-literal`](../reference/rules.md#redundant-literal), off by default, because one
alias often serves several queries that each return some of its values.

## Limitations

- A row type imported from a module outside the checked paths is skipped
  silently.
- A column with neither a cast nor a schema type has an unknown type: the
  query is not wrapped by `rewrite` (`SQL[UNTYPED]` with
  [`untyped = "sentinel"`](untyped.md)), and `check` does not compare the
  column with the field type.
- Validation is static. Nothing checks rows at runtime; use Pydantic if you
  need that.

Example project:
[`examples/04_check_and_types/`](https://github.com/jirikuncar/sqlty/tree/main/examples/04_check_and_types).
