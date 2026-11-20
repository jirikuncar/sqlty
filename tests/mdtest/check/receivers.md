# Receivers of helper methods

A helper method is checked when the class of its receiver is declared, the way a type
checker reads it. Each case names what ty 0.0.107 reveals for the receiver; a receiver
ty leaves `Unknown` is skipped.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

`pkg/__init__.py`:

```python
```

`pkg/db.py`:

```python
from contextlib import asynccontextmanager, contextmanager
from typing import Annotated, AsyncIterator, ClassVar, Iterator, LiteralString, Self
from sqlty import SQL

class Cursor:
    def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

class Transaction:
    def __enter__(self) -> Cursor: ...
    async def __aenter__(self) -> Cursor: ...

class Postgres:
    def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...
    def get_cursor(self) -> Transaction: ...
    def __enter__(self) -> Self: ...
    @property
    def replica(self) -> "Postgres": ...
    @classmethod
    def connect(cls) -> Self: ...

class DB(Postgres):
    pass

class Model:
    db: ClassVar[Postgres]

class Website:
    db: DB

def make() -> DB: ...

def make_untyped():
    return DB()

async def make_later() -> DB: ...

@contextmanager
def transaction() -> Iterator[Cursor]: ...

@asynccontextmanager
async def transaction_later() -> AsyncIterator[Cursor]: ...
```

`pkg/app.py`:

```python
from pkg import db as database
from pkg.db import DB, Website

class Cron:
    website: Website

class App:
    def __init__(self, cron: Cron) -> None:
        self.cron = cron
        self.fallback = None
        self.fallback = DB()
```

## A variable assigned an instance

ty: `DB`. The method is the one `Postgres` defines.

```python
from pkg.db import DB

def setup(url: str) -> None:
    db = DB(url)
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
```

## An attribute a base class declares

ty: `Postgres`.

```python
from pkg.db import Model

class Participant(Model):
    def name(self) -> None:
        # error: [column-type-mismatch]
        self.db.one("SELECT name FROM users", int)

    @classmethod
    def first(cls) -> None:
        # error: [column-type-mismatch]
        cls.db.one("SELECT name FROM users", int)

# error: [column-type-mismatch]
Participant.db.one("SELECT name FROM users", int)
```

## `with ... as` through `__enter__`

ty: `Cursor`.

```python
from pkg.db import Model

class Participant(Model):
    def name(self) -> None:
        with self.db.get_cursor() as cursor:
            # error: [column-type-mismatch]
            cursor.one("SELECT name FROM users", int)

    async def name_later(self) -> None:
        async with self.db.get_cursor() as cursor:
            # error: [column-type-mismatch]
            cursor.one("SELECT name FROM users", int)
```

## `__enter__` returning `Self`

ty: `DB`.

```python
from pkg.db import DB

with DB() as db:
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
```

## An attribute of an annotated parameter

ty: `DB`.

```python
from pkg.db import Website

def render(website: Website) -> None:
    # error: [column-type-mismatch]
    website.db.one("SELECT name FROM users", int)
```

## A chain of attributes across modules

ty: `DB`.

```python
from pkg.app import App

class Job:
    def __init__(self, app: App) -> None:
        self.app = app

    def run(self) -> None:
        # error: [column-type-mismatch]
        self.app.cron.website.db.one("SELECT name FROM users", int)
        # error: [column-type-mismatch]
        self.app.fallback.one("SELECT name FROM users", int)
```

## What no declaration states is skipped

ty: `Unknown` for each, `None | Unknown` for `cls.db`.

```python
from pkg.db import DB, make_untyped

class Payday:
    db = None

    @classmethod
    def run(cls) -> None:
        cls.db.one("SELECT name FROM users", int)

def unannotated(db):
    db.one("SELECT name FROM users", int)

def returned() -> None:
    db = make_untyped()
    db.one("SELECT name FROM users", int)

def unpacked(pair) -> None:
    db, _ = pair
    db.one("SELECT name FROM users", int)
    for db in pair:
        db.one("SELECT name FROM users", int)

# error:
```

## Attributes assigned in the class

ty: `DB` for each.

```python
from pkg.db import DB

class Service:
    shared = DB()

    def __init__(self, given: DB) -> None:
        self.db = DB()
        self.given = given

    def run(self) -> None:
        # error: [column-type-mismatch]
        self.db.one("SELECT name FROM users", int)
        # error: [column-type-mismatch]
        self.shared.one("SELECT name FROM users", int)
        # error: [column-type-mismatch]
        self.given.one("SELECT name FROM users", int)
```

## Annotated returns

ty: `DB`, and `Cursor` in the `with`.

```python
from typing import cast
import pkg.db
from pkg.db import DB, make, make_later, transaction, transaction_later

async def run(untyped) -> None:
    # error: [column-type-mismatch]
    make().one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    pkg.db.make().one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    DB.connect().one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    make().replica.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    cast(DB, untyped).one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    cast("DB", untyped).one("SELECT name FROM users", int)
    later = await make_later()
    # error: [column-type-mismatch]
    later.one("SELECT name FROM users", int)
    with transaction() as cursor:
        # error: [column-type-mismatch]
        cursor.one("SELECT name FROM users", int)
    async with transaction_later() as cursor:
        # error: [column-type-mismatch]
        cursor.one("SELECT name FROM users", int)
```

## `T | None` is `T`

ty: `DB | None`, and reports the call; sqlty checks the query.

```python
from typing import Optional
from pkg.db import DB

class Service:
    maybe: DB | None
    older: Optional[DB] = None

    def run(self, given: "DB | None" = None) -> None:
        # error: [column-type-mismatch]
        self.maybe.one("SELECT name FROM users", int)
        # error: [column-type-mismatch]
        self.older.one("SELECT name FROM users", int)
        # error: [column-type-mismatch]
        given.one("SELECT name FROM users", int)
```

## A union of classes is unknown

```python
from pkg.db import DB, Cursor

def run(either: DB | Cursor) -> None:
    either.one("SELECT name FROM users", int)

# error:
```

## A name is what it was last bound to

ty: `Cursor` inside the `with`, `DB` before; `DB | Cursor` after the `if`.

```python
from pkg.db import DB, make

def run(db=None, flag: bool = False) -> None:
    db = make()
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
    with db.get_cursor() as db:
        # error: [column-type-mismatch]
        db.one("SELECT name FROM users", int)
    other = make()
    if flag:
        other = other.get_cursor().__enter__()
    other.one("SELECT name FROM users", int)
```

## A method the class defines hides the helper of its base

ty: `bound method Quiet.one`.

```python
from pkg.db import DB

class Quiet(DB):
    def one(self, query, form):
        return None

class Loud(DB):
    pass

def run(quiet: Quiet, loud: Loud) -> None:
    quiet.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    loud.one("SELECT name FROM users", int)
```

## `isinstance` does not narrow yet

ty: `DB`. sqlty skips the call.

```python
from pkg.db import DB

def run(x: object) -> None:
    if isinstance(x, DB):
        x.one("SELECT name FROM users", int)

# error:
```

## A registered method applies to subclasses

```toml
[[tool.sqlty.functions]]
import-path = "lib.Postgres.one"
query-param = "sql"
return-type-param = "form"
```

The library is not read here: the class is known by the path it is imported by, and the
query is passed by name as the parameters of the method are not known.

```python
from lib import Postgres

class DB(Postgres):
    pass

class Website:
    db: DB

def run(website: Website) -> None:
    # error: [column-type-mismatch]
    website.db.one(sql="SELECT name FROM users", form=int)
```

## Stubs

`typings/lib/__init__.pyi`:

```python
from typing import Annotated, Any, LiteralString
from sqlty import SQL
from .cursors import Cursor
from contextlib import AbstractContextManager

class Postgres:
    def one[T](self, sql: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...
    def get_cursor(self) -> AbstractContextManager[Cursor]: ...
```

`typings/lib/cursors.pyi`:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Cursor:
    def one[T](self, sql: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...
```

`typings/lib/orm.pyi`:

```python
from typing import ClassVar
from . import Postgres

class Model:
    db: ClassVar[Postgres]
```

### A stub marks the helpers of a library

ty with the stub: `Postgres`, and `Cursor` in the `with`.

```python
from lib.orm import Model

class Participant(Model):
    def name(self) -> None:
        # error: [column-type-mismatch]
        self.db.one("SELECT name FROM users", int)
        with self.db.get_cursor() as cursor:
            # error: [column-type-mismatch]
            cursor.one("SELECT name FROM users", int)
```

### A stub is read before the module

`lib/__init__.py`:

```python
class Postgres:
    def one(self, sql, form=None): ...

class DB:
    pass
```

```python
from lib import Postgres, DB

def run(db: Postgres, other: DB) -> None:
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
    other.one("SELECT name FROM users", int)
```
