# TypedDict Inheritance of Closed and extra_items Bases

PEP 728 restricts subclasses of closed and `extra_items` TypedDicts.
Violations are `typeddict-inheritance` errors reported on the subclass's
`class` line, for TypedDicts used as `SQL[T]` row types and their bases.
Messages name the base whose header declared the openness. A subclass that
tries to reopen or change a base's openness keeps the base's, so its keys are
still matched against the query.

Types are compared only when the answer is certain: plain classes and unions
of them, with `int` -> `float` -> `complex` promotion. Generics, literals and
other types are given the benefit of the doubt. Only bases defined in the same
file and spelled as bare names are checked.

```toml
[tool.sqlty]
dialect = "postgres"
```

## Closed base — adding a key

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
class User(Base):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Closed base — adding a NotRequired key

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
class User(Base):
    name: NotRequired[str]

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
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

# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
class User(Base):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Grandchild of an invalid subclass reports the subclass once

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

# error: [typeddict-inheritance] "Middle cannot add key 'name' to closed base Base"
class Middle(Base):
    name: str

class User(Middle):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
r = SQL[Middle]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Closed base reached through a pass-through subclass names the declaring base

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class Middle(Base):
    pass

# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
class User(Middle):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Multiple inheritance — keys from a sibling base

A closed base admits no keys from an open sibling, in either base order.

```python
from typing import TypedDict
from sqlty import SQL

class Closed(TypedDict, closed=True):
    id: int

class Open(TypedDict):
    name: str

# error: [typeddict-inheritance] "OpenFirst cannot add key 'name' to closed base Closed"
class OpenFirst(Open, Closed):
    pass

# error: [typeddict-inheritance] "ClosedFirst cannot add key 'email' to closed base Closed"
# error: [typeddict-inheritance] "ClosedFirst cannot add key 'name' to closed base Closed"
class ClosedFirst(Closed, Open):
    email: str

q = SQL[OpenFirst]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
r = SQL[ClosedFirst]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
```

## Multiple inheritance — one message per key and declaring base

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class Left(Base):
    pass

class Right(Base):
    pass

# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
class User(Left, Right):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Closed base — closed=False cannot reopen

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    name: NotRequired[str]

# error: [typeddict-inheritance] "User cannot set closed=False: base Base is closed"
class User(Base, closed=False):
    pass

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## Closed base — closed=False without runtime resolution

```python
import not_installed
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    name: NotRequired[str]

# error: [typeddict-inheritance] "User cannot set closed=False: base Base is closed"
class User(Base, closed=False):
    pass

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## Closed base — extra_items cannot reopen

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int
    name: NotRequired[str]

# error: [typeddict-inheritance] "User cannot set extra_items=str: base Base is closed"
class User(Base, extra_items=str):
    pass

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## extra_items base — closed=False cannot reopen

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    id: int

# error: [typeddict-inheritance] "User cannot set closed=False: base Base has extra_items"
class User(Base, closed=False):
    pass

# error: [column-type-mismatch] "User.ratio is extra_items int but query column 'ratio' infers float"
q = SQL[User]("--sql\nSELECT id::INTEGER, ratio::FLOAT FROM users")
```

## Mutable extra_items base — new key must be NotRequired

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    id: int

# error: [typeddict-inheritance] "User.views must be NotRequired: base Base has extra_items int"
class User(Base):
    views: int

q = SQL[User]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
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

# error: [typeddict-inheritance] "User.views is int but base Base has extra_items int | None"
class User(Base):
    views: NotRequired[int]

q = SQL[User]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
```

## Mutable extra_items base — cannot close or change extra_items

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    id: int

# error: [typeddict-inheritance] "Sealed cannot set closed=True: base Base has mutable extra_items int"
class Sealed(Base, closed=True):
    pass

# error: [typeddict-inheritance] "Flags cannot set extra_items=bool: base Base has mutable extra_items int"
class Flags(Base, extra_items=bool):
    pass

q = SQL[Sealed]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
r = SQL[Flags]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
```

## Mutable extra_items base — cannot become read-only

```python
from typing import ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int):
    id: int

# error: [typeddict-inheritance] "User cannot set extra_items=ReadOnly[int]: base Base has mutable extra_items int"
class User(Base, extra_items=ReadOnly[int]):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
```

## extra_items type checks without runtime resolution

Builtin scalar types and their unions are compared from source.

```python
import not_installed
from typing import NotRequired, ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=int | None):
    id: int

# error: [typeddict-inheritance] "User.views is int but base Base has extra_items int | None"
class User(Base):
    views: NotRequired[int]

# error: [typeddict-inheritance] "Flags cannot set extra_items=bool: base Base has mutable extra_items int | None"
class Flags(Base, extra_items=bool):
    pass

class Scores(TypedDict, extra_items=ReadOnly[int | str]):
    id: int

# error: [typeddict-inheritance] "Ranked.score is float but base Scores has read-only extra_items int | str"
class Ranked(Scores):
    score: float

q = SQL[User]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
r = SQL[Flags]("--sql\nSELECT id::INTEGER FROM users")
s = SQL[Ranked]("--sql\nSELECT id::INTEGER, score::FLOAT FROM users")
```

## Read-only extra_items base — undecidable types are not flagged

```python
from collections.abc import Sequence
from typing import Literal, NotRequired, ReadOnly, TypedDict
from sqlty import SQL

class Ratio(TypedDict, extra_items=ReadOnly[float]):
    id: int

class Count(Ratio):
    views: int

class Items(TypedDict, extra_items=ReadOnly[Sequence[int]]):
    id: int

class Tags(Items):
    tags: NotRequired[list[int]]

class Text(TypedDict, extra_items=ReadOnly[str]):
    id: int

class Kind(Text):
    kind: NotRequired[Literal["x"]]

class Forward(TypedDict, extra_items=ReadOnly["int"]):
    id: int

class Views(Forward):
    views: int

q = SQL[Count]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
r = SQL[Tags]("--sql\nSELECT id::INTEGER FROM users")
s = SQL[Kind]("--sql\nSELECT id::INTEGER FROM users")
t = SQL[Views]("--sql\nSELECT id::INTEGER, views::INTEGER FROM users")
# error:
```

## extra_items base — Any in a union is not flagged

```python
from typing import Any, NotRequired, ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=ReadOnly[int]):
    id: int

class Child(Base):
    x: NotRequired[int | Any]

class Mutable(TypedDict, extra_items=int):
    id: int

class Loose(Mutable):
    x: NotRequired[int | Any]

class Wide(TypedDict, extra_items=ReadOnly[int | Any]):
    id: int

class Score(Wide):
    score: float

class Top(TypedDict, extra_items=ReadOnly[object]):
    id: int

class Name(Top):
    name: NotRequired[str | Any]

q = SQL[Child]("--sql\nSELECT id::INTEGER FROM users")
r = SQL[Loose]("--sql\nSELECT id::INTEGER FROM users")
s = SQL[Score]("--sql\nSELECT id::INTEGER, score::FLOAT FROM users")
t = SQL[Name]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

## extra_items base — Any in a union is not flagged without runtime resolution

```python
import not_installed
from typing import Any, NotRequired, ReadOnly, TypedDict
from sqlty import SQL

class Base(TypedDict, extra_items=ReadOnly[int]):
    id: int

class Child(Base):
    x: NotRequired[int | Any]

class Mutable(TypedDict, extra_items=int):
    id: int

class Loose(Mutable):
    x: NotRequired[int | Any]

class Wide(TypedDict, extra_items=ReadOnly[int | Any]):
    id: int

class Score(Wide):
    score: float

class Top(TypedDict, extra_items=ReadOnly[object]):
    id: int

class Name(Top):
    name: NotRequired[str | Any]

q = SQL[Child]("--sql\nSELECT id::INTEGER FROM users")
r = SQL[Loose]("--sql\nSELECT id::INTEGER FROM users")
s = SQL[Score]("--sql\nSELECT id::INTEGER, score::FLOAT FROM users")
t = SQL[Name]("--sql\nSELECT id::INTEGER FROM users")
# error:
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

# error: [typeddict-inheritance] "User.score is float but base Base has read-only extra_items int | str"
class User(Base):
    score: float

q = SQL[User]("--sql\nSELECT id::INTEGER, score::FLOAT FROM users")
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

# error: [typeddict-inheritance] "User cannot set closed=False: base Base has extra_items"
class User(Base, closed=False):
    pass

q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
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

## Imported bases are not checked

Only bases defined in the same file are known, so a subclass of an imported
closed TypedDict is not checked.

```python
from models import Base
from sqlty import SQL

class User(Base):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## typeddict-inheritance is configurable

```toml
[tool.sqlty.rules]
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
