# TypedDict Inheritance of Closed and extra_items Bases

PEP 728 restricts subclasses of closed and `extra_items` TypedDicts.
Violations are `typeddict-inheritance` errors reported on the subclass, for
TypedDicts used as `SQL[T]` row types and their bases. A subclass that tries
to reopen or change a base's openness keeps the base's, so its keys are still
matched against the query.

```toml
[sqlty]
dialect = "postgres"
```

## Closed base — adding a key

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class User(Base):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
```

## Closed base — adding a NotRequired key

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class User(Base):
    name: NotRequired[str]

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
```

## Closed base — redeclaring a key and closed=True are allowed

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class User(Base, closed=True):
    id: int

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

## extra_items=Never base — adding a key

```python
from typing import Never, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=Never):
    id: int

class User(Base):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
```

## Grandchild of an invalid subclass reports the subclass once

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class Middle(Base):
    name: str

class User(Middle):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
r = SQL[Middle]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [typeddict-inheritance] "Middle cannot add key 'name' to closed base Base"
```

## Closed base — closed=False cannot reopen

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    name: NotRequired[str]

class User(Base, closed=False):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
# error: [typeddict-inheritance] "User cannot set closed=False: base Base is closed"
# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
```

## Closed base — closed=False without runtime resolution

```python
import not_installed
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    name: NotRequired[str]

class User(Base, closed=False):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
# error: [typeddict-inheritance] "User cannot set closed=False: base Base is closed"
# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
```

## Closed base — extra_items cannot reopen

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    name: NotRequired[str]

class User(Base, extra_items=str):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
# error: [typeddict-inheritance] "User must stay closed: base Base is closed"
# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
```

## extra_items base — closed=False cannot reopen

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    id: int

class User(Base, closed=False):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER, ratio::FLOAT FROM users")
# error: [typeddict-inheritance] "User cannot set closed=False: base Base has extra_items"
# error: [column-type-mismatch] "User.ratio is extra_items int but query column 'ratio' infers float"
```

## Mutable extra_items base — new key must be NotRequired

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    id: int

class User(Base):
    views: int

q = SQL[User]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
# error: [typeddict-inheritance] "User.views must be NotRequired: base Base has extra_items int"
```

## Mutable extra_items base — NotRequired key of the same type is allowed

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    id: int

class User(Base):
    views: NotRequired[int]

q = SQL[User]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
# error:
```

## Mutable extra_items base — new key type must be consistent

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int | None):
    id: int

class User(Base):
    views: NotRequired[int]

q = SQL[User]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
# error: [typeddict-inheritance] "User.views is int but base Base has extra_items int | None"
```

## Mutable extra_items base — cannot close or change extra_items

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    id: int

class Sealed(Base, closed=True):
    pass

class Flags(Base, extra_items=bool):
    pass

q = SQL[Sealed]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
r = SQL[Flags]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
# error: [typeddict-inheritance] "Sealed must keep base Base extra_items int"
# error: [typeddict-inheritance] "Flags extra_items bool doesn't fit base Base extra_items int"
```

## Read-only extra_items base — required key of an assignable type is allowed

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=ReadOnly[int | str]):
    id: int

class User(Base):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Read-only extra_items base — new key type must be assignable

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=ReadOnly[int | str]):
    id: int

class User(Base):
    score: float

q = SQL[User]("--sql\nSELECT id::INTEGER, score::FLOAT FROM users")
# error: [typeddict-inheritance] "User.score is float but base Base has read-only extra_items int | str"
```

## Read-only extra_items base — may narrow or close

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=ReadOnly[int | str]):
    id: int

class Names(Base, extra_items=str):
    pass

class Sealed(Base, closed=True):
    pass

q = SQL[Names]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
r = SQL[Sealed]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

## Read-only extra_items base — closed=False cannot reopen

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=ReadOnly[int]):
    id: int

class User(Base, closed=False):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error: [typeddict-inheritance] "User cannot set closed=False: base Base has extra_items"
```

## Open base — subclasses may close, add keys and set closed=False

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict):
    id: int

class Open(Base, closed=False):
    name: str

class User(Open, closed=True):
    email: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
# error:
```

## typeddict-inheritance is configurable

```toml
[sqlty.rules]
typeddict-inheritance = "ignore"
```

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class User(Base, closed=False):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```
