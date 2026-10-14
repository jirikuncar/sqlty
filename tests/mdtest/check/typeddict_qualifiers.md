# TypedDict Qualifiers and Closed TypedDicts

Without a `sql` block, the Python block is checked as a whole, so the
TypedDict defined there is validated against the query.

Qualifiers (`ReadOnly`, `Required`, `NotRequired`) are stripped before
field types are compared. Open TypedDicts match columns by position.
Closed TypedDicts (`closed=True`) and those with `extra_items=T` match by
key: `NotRequired` keys may be absent, extra columns are only allowed with
`extra_items` and must match `T`.
`extra_items=Never` means `closed=True`, and subclasses inherit both from
their bases (PEP 728). Key-set violations are `typeddict-key-mismatch`.

```toml
[tool.sqlty]
dialect = "postgres"
```

## ReadOnly fields match

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: ReadOnly[int]
    name: ReadOnly[str | None]

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## ReadOnly field type mismatch reports the bare type

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: ReadOnly[str]

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
```

## Required and NotRequired nested with ReadOnly

```python
import typing
from typing import NotRequired as Opt
from sqlty import SQL

class User(typing.TypedDict, total=False):
    id: typing.Required[typing.ReadOnly[int]]
    name: Opt[typing.ReadOnly[str]]

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Qualifiers are stripped without runtime resolution

The unresolvable import forces the AST fallback.

```python
import typing as t
import not_installed
from sqlty import SQL

class User(t.TypedDict):
    id: t.ReadOnly[int]
    name: t.NotRequired[t.ReadOnly[str]]

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Open TypedDict may omit NotRequired keys

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: NotRequired[str]

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

## Closed — exact match

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str

q = SQL[User]("--sql\nSELECT name::TEXT, id::INTEGER FROM users")
# error:
```

## Closed — extra column

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str

# error: [column-count-mismatch] "User has 2 fields but query returns 3 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
```

## Closed — NotRequired key may be absent

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    email: NotRequired[str]

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

## Closed — undeclared column instead of a NotRequired key

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    email: NotRequired[str]

# error: [typeddict-key-mismatch] "User has no key for query column 'name'"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Closed — missing required key

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str
    email: NotRequired[str]

# error: [typeddict-key-mismatch] "User.name is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## extra_items — typed extras allowed

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=ReadOnly[int]):
    day: str

q = SQL[Metrics]("--sql\nSELECT day::TEXT, views::INTEGER, clicks::INTEGER FROM stats")
# error:
```

## extra_items — extra column type mismatch

```python
from typing import TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=int):
    day: str

# error: [column-type-mismatch] "Metrics.ratio is extra_items int but query column 'ratio' infers float"
q = SQL[Metrics]("--sql\nSELECT day::TEXT, ratio::FLOAT FROM stats")
```

## extra_items — missing required key

```python
from typing import TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=int):
    day: str

# error: [typeddict-key-mismatch] "Metrics.day is required but missing from query"
q = SQL[Metrics]("--sql\nSELECT views::INTEGER FROM stats")
```

## extra_items — type mismatch without runtime resolution

```python
import not_installed
from typing import TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=int):
    day: str

# error: [column-type-mismatch] "Metrics.ratio is extra_items int but query column 'ratio' infers float"
q = SQL[Metrics]("--sql\nSELECT day::TEXT, ratio::FLOAT FROM stats")
```

## extra_items=Never is closed

```python
from typing import Never, TypedDict
from sqlty import SQL

class User(TypedDict, extra_items=Never):
    id: int

# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## extra_items=NoReturn is closed

```python
import typing
from sqlty import SQL

class User(typing.TypedDict, extra_items=typing.NoReturn):
    id: int
    name: typing.NotRequired[str]

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## extra_items=Never is closed without runtime resolution

```python
import not_installed
from typing import Never as Nothing, NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict, extra_items=Nothing):
    id: int
    name: NotRequired[str]

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## Subclass of a closed TypedDict is closed

Subclasses may not add keys to a closed base, but may make a `ReadOnly`
key required.

```python
from typing import NotRequired, ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    note: NotRequired[ReadOnly[str]]

class User(Base):
    note: ReadOnly[str]

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.note is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## Subclass without own fields inherits keys and closedness

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    name: NotRequired[str]

class User(Base):
    pass

# error: [typeddict-key-mismatch] "User.id is required but missing from query"
q = SQL[User]("--sql\nSELECT name::TEXT FROM users")
```

## Subclass of a closed TypedDict without runtime resolution

```python
import not_installed
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    name: NotRequired[str]

class Middle(Base):
    pass

class User(Middle):
    pass

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.id is required but missing from query"
q = SQL[User]("--sql\nSELECT name::TEXT, email::TEXT FROM users")
```

## Subclass closes a read-only extra_items base

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=ReadOnly[str]):
    id: int

class User(Base, closed=True):
    name: str

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## Subclass inherits extra_items

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    day: str

class Metrics(Base):
    pass

# error: [column-type-mismatch] "Metrics.ratio is extra_items int but query column 'ratio' infers float"
q = SQL[Metrics]("--sql\nSELECT day::TEXT, ratio::FLOAT FROM stats")
```

## Subclass of an open TypedDict stays open

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict):
    id: int

class User(Base):
    name: str

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## typeddict-key-mismatch is configurable

```toml
[tool.sqlty.rules]
typeddict-key-mismatch = "ignore"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
# error:
```
