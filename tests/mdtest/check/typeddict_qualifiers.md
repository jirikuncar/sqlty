# TypedDict Qualifiers and Closed TypedDicts

Without a `sql` block, the Python block is checked as a whole, so the
TypedDict defined there is validated against the query.

Qualifiers (`ReadOnly`, `Required`, `NotRequired`) are stripped before
field types are compared. Open TypedDicts match columns by position.
Closed TypedDicts (`closed=True`) and those with `extra_items=T` match by
key: `NotRequired` keys may be absent, extra columns are only allowed with
`extra_items` and must match `T`.

```toml
[sqlty]
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

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
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

## Open TypedDict still requires NotRequired keys positionally

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: NotRequired[str]

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error: [column-count-mismatch] "User has 2 fields but query returns 1 columns"
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

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
# error: [column-count-mismatch] "User has 2 fields but query returns 3 columns"
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

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-name-mismatch] "User has no key for query column 'name'"
```

## Closed — missing required key

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str
    email: NotRequired[str]

q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
# error: [column-name-mismatch] "User.name is required but missing from query"
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

q = SQL[Metrics]("--sql\nSELECT day::TEXT, ratio::FLOAT FROM stats")
# error: [column-type-mismatch] "Metrics.ratio is extra_items int but query column 'ratio' infers float"
```

## extra_items — missing required key

```python
from typing import TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=int):
    day: str

q = SQL[Metrics]("--sql\nSELECT views::INTEGER FROM stats")
# error: [column-name-mismatch] "Metrics.day is required but missing from query"
```

## extra_items — type mismatch without runtime resolution

```python
import not_installed
from typing import TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=int):
    day: str

q = SQL[Metrics]("--sql\nSELECT day::TEXT, ratio::FLOAT FROM stats")
# error: [column-type-mismatch] "Metrics.ratio is extra_items int but query column 'ratio' infers float"
```
