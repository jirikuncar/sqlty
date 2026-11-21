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

### An `async def` gives its value once awaited

Calling an `async def` gives a coroutine, which has none of the methods of what it
returns. Only `async with` enters an `@asynccontextmanager`, and only `with` a
`@contextmanager`. ty: `CoroutineType[Any, Any, DB]`, and an error on each `with`.

```python
from pkg.db import make_later, transaction, transaction_later

async def run() -> None:
    make_later().one("SELECT name FROM users", int)
    later = make_later()
    later.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    (await later).one("SELECT name FROM users", int)
    with transaction_later() as cursor:
        cursor.one("SELECT name FROM users", int)
    async with transaction() as cursor:
        cursor.one("SELECT name FROM users", int)
```

### A type parameter of the function in what it returns

A type parameter of a function is a type of its own, whatever class of that name the
module has: it is unknown in what the function returns, and the rest of the annotation
is kept, so `Cursor[T]` is a `Cursor`. ty: the type of the argument.

```python
from typing import Optional
from pkg.db import DB

class T(DB): ...

def first[T](item: T) -> T: ...

def all_of[T](items: list[T]) -> "T | None": ...

def maybe[T](item: T) -> Optional[T]: ...

class Cursor[R](DB): ...

def cursor[T](row: type[T]) -> Cursor[T]: ...

class Box:
    def take[T](self, item: T) -> T: ...

def plain(item: object) -> T: ...

def run(box: Box) -> None:
    first(object()).one("SELECT name FROM users", int)
    all_of([]).one("SELECT name FROM users", int)
    maybe(object()).one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    cursor(int).one("SELECT name FROM users", int)
    box.take(object()).one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    plain(object()).one("SELECT name FROM users", int)
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

### An annotation nested too deep, and one that is not a name

An annotation is read eight levels deep. A subscript of something that is not a dotted
name states nothing.

```python
from typing import Optional
from pkg.db import DB, make

def eight(db: Optional[Optional[Optional[Optional[Optional[Optional[Optional[Optional[DB]]]]]]]]) -> None:
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    db.one("SELECT name FROM users", int)

def nine(db: Optional[Optional[Optional[Optional[Optional[Optional[Optional[Optional[Optional[DB]]]]]]]]]) -> None:
    db.one("SELECT name FROM users", int)

def computed(db: make()[DB]) -> None:
    db.one("SELECT name FROM users", int)
```

### A chain of attributes too long

```python
from pkg.db import DB

class Node:
    next: "Node"
    db: DB

def run(node: Node) -> None:
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    node.next.next.next.next.next.next.next.next.next.next.next.next.next.next.next.db.one("SELECT name FROM users", int)
    node.next.next.next.next.next.next.next.next.next.next.next.next.next.next.next.next.db.one("SELECT name FROM users", int)
```

### What cannot be called, entered or awaited

A special form of `typing` and an instance give nothing when called, a class nothing
when entered, and a `@contextmanager` function that is not annotated with what it yields
enters nothing that is known. `type[type[DB]]` is not followed, and neither is a name
that is a class by one binding and an instance by another, nor an attribute no class
declares. A value that is awaited without being what an `async def` gives is itself.

```python
from contextlib import contextmanager
from typing import Optional
from pkg.db import DB, Cursor, Website

Mixed = DB
Mixed = DB()

@contextmanager
def bare() -> Cursor: ...

async def run(db: DB, site: Website, deep: type[type[DB]], mixed: Mixed) -> None:
    Optional(db).one("SELECT name FROM users", int)
    db().one("SELECT name FROM users", int)
    site.nothing.one("SELECT name FROM users", int)
    with bare() as cursor:
        cursor.one("SELECT name FROM users", int)
    with DB as entered:
        entered.one("SELECT name FROM users", int)
    deep().one("SELECT name FROM users", int)
    mixed.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    (await db).one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    (made := DB()).one("SELECT name FROM users", int)
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

### An annotated parameter is one of the values assigned

A parameter assigned to an attribute gives it the class it is annotated with, as any
other value would: unlike `self.db: DB`, it does not declare what the attribute holds.
An attribute another method assigns something else to is either, and its call is not
checked. ty: `DB | Plain` for the first, `DB | None` for the second.

```python
from pkg.db import DB

class Plain:
    def one(self, query, row): ...

class Service:
    def __init__(self, given: DB) -> None:
        self.db = given

    def close(self) -> None:
        self.db = Plain()

    def run(self) -> None:
        # error:
        self.db.one("SELECT name FROM users", int)

class Closing:
    def __init__(self, given: DB) -> None:
        self.db = given

    def close(self) -> None:
        self.db = None

    def run(self) -> None:
        # error: [column-type-mismatch]
        self.db.one("SELECT name FROM users", int)

class Declared:
    db: DB

    def __init__(self, given: DB) -> None:
        self.db = given

    def close(self) -> None:
        self.db = Plain()

    def run(self) -> None:
        # error: [column-type-mismatch]
        self.db.one("SELECT name FROM users", int)
```

### A value written in the body of the class

An attribute is what its value is in the body of the class: an attribute of another
name, a call, a `cast`, what a `with` enters. A value that names something the body
binds is not followed.

```python
from typing import cast
from pkg import db as database
from pkg.db import DB, Cursor, Website, make, transaction

site: Website

class Holder:
    DB = Cursor
    db = site.db
    made = database.make()
    kept = cast(Website, make())
    hidden = cast(DB, make())
    local = DB.connect()
    with transaction() as cursor:
        pass
    with DB() as own:
        pass

def run(holder: Holder) -> None:
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    holder.db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    holder.made.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    holder.kept.db.one("SELECT name FROM users", int)
    holder.hidden.one("SELECT name FROM users", int)
    holder.local.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    holder.cursor.one("SELECT name FROM users", int)
    holder.own.one("SELECT name FROM users", int)
```

### A value a method assigns

An attribute a method assigns is what the value is in the method: an attribute of
`self`, what a `with` enters, what is awaited. A name the method binds more than once is
not followed, a `@staticmethod` has no receiver, and an attribute of another parameter
is not one of the class.

```python
from pkg.db import DB, Cursor, Website, make_later, transaction

class App:
    def __init__(self, site: Website, db: Cursor) -> None:
        self.site = site
        self.db = self.site.db
        db = DB()
        self.twice = db

    def open(self) -> None:
        with transaction() as self.cursor:
            pass

    async def later(self) -> None:
        self.made = await make_later()

    @staticmethod
    def build(self) -> None:
        self.static = DB()

    def other(self, app: "App") -> None:
        app.elsewhere = DB()

def run(app: App) -> None:
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    app.db.one("SELECT name FROM users", int)
    app.twice.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    app.cursor.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    app.made.one("SELECT name FROM users", int)
    app.static.one("SELECT name FROM users", int)
    app.elsewhere.one("SELECT name FROM users", int)
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

### A decorator that is called, and a setter

A decorator that is called is the one it calls. The `setter` and `deleter` of a property
do not say what it returns, and a function one of them replaces is not a function.

```python
import functools
from contextlib import contextmanager
from typing import Iterator
from pkg.db import DB, Cursor

@functools.lru_cache(maxsize=None)
def shared() -> DB: ...

@contextmanager()
def entered() -> Iterator[Cursor]: ...

class Holder:
    @property
    def db(self) -> DB: ...

    @db.setter
    def db(self, value: Cursor) -> None: ...

@Holder.db.setter
def replace(holder: Holder) -> DB: ...

def run(holder: Holder) -> None:
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    shared().one("SELECT name FROM users", int)
    with entered() as cursor:
        # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
        cursor.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    holder.db.one("SELECT name FROM users", int)
    replace(holder).one("SELECT name FROM users", int)
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

## A union of classes

A receiver that is of any of several classes is checked when each class reaches a helper
and they take the query, the row type and the parameters the same way. ty: `DB | Cursor`.

```python
from pkg.db import DB, Cursor

def run(either: DB | Cursor) -> None:
    # error: [column-type-mismatch]
    either.one("SELECT name FROM users", int)
```

A class without the helper, or with one that takes the call another way, leaves the call
unchecked. ty: `DB | Website`, `DB | Reader`.

`pkg/reader.py`:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Reader:
    def one[T](self, form: type[T], query: Annotated[LiteralString, SQL], /) -> T | None: ...

class Deep:
    reader: Reader
```

```python
from pkg.db import DB, Website
from pkg.reader import Reader

def run(either: DB | Website, other: DB | Reader) -> None:
    either.one("SELECT name FROM users", int)
    other.one("SELECT name FROM users", int)

# error:
```

An attribute of the union is the union of the attributes, and a call of it of what each
gives. ty: `DB | Postgres`.

```python
from pkg.db import Model, Website, make

def run(either: Website | Model, made: type[Website] | type[Model]) -> None:
    # error: [column-type-mismatch]
    either.db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    made().db.one("SELECT name FROM users", int)
```

More than 8 classes are not followed.

```python
from pkg.db import DB

class A1(DB): ...
class A2(DB): ...
class A3(DB): ...
class A4(DB): ...
class A5(DB): ...
class A6(DB): ...
class A7(DB): ...
class A8(DB): ...
class A9(DB): ...

def eight(db: A1 | A2 | A3 | A4 | A5 | A6 | A7 | A8) -> None:
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)

def nine(db: A1 | A2 | A3 | A4 | A5 | A6 | A7 | A8 | A9) -> None:
    db.one("SELECT name FROM users", int)
```

### A member that is itself any of several classes

A name that is any of several classes gives each of them to the union it is a member of,
once.

```python
from pkg.db import DB, Cursor

try:
    from pkg.db import Postgres as Base
except ImportError:
    from pkg.db import DB as Base

def run(either: Base | Cursor, other: DB | Base) -> None:
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    either.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    other.one("SELECT name FROM users", int)
```

### A union that is awaited, entered or called

`await`, `with` and a call give what each member gives. A member that cannot be entered
leaves what `with` binds unknown, and so does `type[...]` of a member that is not a class.

```python
from pkg.db import DB, Cursor, Postgres, Transaction, Website, make_later

async def run(
    flag: bool,
    managers: Transaction | Postgres,
    mixed: Transaction | Website,
    made: type[DB | Cursor],
    odd: type[DB | type[Cursor]],
) -> None:
    if flag:
        later = make_later()
    else:
        later = DB()
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    (await later).one("SELECT name FROM users", int)
    with managers as entered:
        # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
        entered.one("SELECT name FROM users", int)
    with mixed as unknown:
        unknown.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    made().one("SELECT name FROM users", int)
    odd().one("SELECT name FROM users", int)
```

### `cast` imported with a fallback

Either `cast` is `cast`.

```python
from pkg.db import DB

try:
    from typing import cast
except ImportError:
    from typing_extensions import cast

def run(value: object) -> None:
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    cast(DB, value).one("SELECT name FROM users", int)
```

## A name bound once

A name its scope binds once, as a parameter or by a statement of its own, holds that
binding at every use after it. A use before it holds nothing, code after a `return` is
not reached, and a lambda written before it may run after it.

```python
from pkg.db import DB

def after(flags) -> None:
    db = DB()
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
    for _ in flags:
        # error: [column-type-mismatch]
        db.one("SELECT name FROM users", int)

def later(flags) -> None:
    # error: [column-type-mismatch]
    early = lambda: db.one("SELECT name FROM users", int)
    db = DB()
    # error: [column-type-mismatch]
    late = [db.one("SELECT name FROM users", int) for _ in flags]
```

```python
from pkg.db import DB

def before(flags) -> None:
    db.one("SELECT name FROM users", int)
    for _ in flags:
        db.one("SELECT name FROM users", int)
    db = DB()

def dead(db: DB) -> None:
    return
    db.one("SELECT name FROM users", int)

def shadowed(db: DB, others) -> None:
    [db.one("SELECT name FROM users", int) for db in others]

# error:
```

## A lambda or a comprehension in the body of a class

The body of a lambda and a comprehension run in a scope of their own, which does not see
the names of the class around, as a method does not: `db` is there the name of the
module. Only the iterable of the first `for` is read in the class.

```python
from pkg.db import DB

class Plain:
    def one(self, query, row): ...

db = Plain()

class Body:
    db = DB()
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    first = [row for row in db.one("SELECT name FROM users", int)]
    # error: [column-type-mismatch]
    default = lambda found=db.one("SELECT name FROM users", int): found
```

```python
from pkg.db import DB

class Plain:
    def one(self, query, row): ...

db = Plain()

class Body:
    db = DB()
    later = lambda: db.one("SELECT name FROM users", int)
    each = [db.one("SELECT name FROM users", int) for _ in range(3)]
    some = {db.one("SELECT name FROM users", int) for _ in range(3)}
    keyed = {n: db.one("SELECT name FROM users", int) for n in range(3)}
    lazy = (db.one("SELECT name FROM users", int) for _ in range(3))
    second = [n for _ in range(3) for n in db.one("SELECT name FROM users", int)]
    kept = [n for n in range(3) if db.one("SELECT name FROM users", int)]

# error:
```

A lambda runs when it is called, so it reads the name of the module as a method does,
whatever the order of the statements: here `db` may be the `Plain` assigned after the
class. A comprehension runs where it is written.

```python
from pkg.db import DB

class Plain:
    def one(self, query, row): ...

db = DB()

class Body:
    later = lambda: db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    each = [db.one("SELECT name FROM users", int) for _ in range(3)]
    inner = lambda: [db.one("SELECT name FROM users", int) for _ in range(3)]

db = Plain()
```

A name the class does not hide is the module's there as anywhere.

```python
from pkg.db import DB

db = DB()

class Body:
    # error: [column-type-mismatch]
    later = lambda: db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    each = [db.one("SELECT name FROM users", int) for _ in range(3)]
```

## The decorators and bases of a class

The decorators, the bases and the keywords of a class statement are read before its
body runs, in the scope around it: a name the body binds is there what the module, or
the function around, binds it to.

```python
from pkg.db import DB

class Plain:
    def one(self, query, row): ...

def deco(*args): ...
def base(*args): ...

db = DB()

# error: [column-type-mismatch]
@deco(db.one("SELECT name FROM users", int))
# error: [column-type-mismatch]
class Hides(base(db.one("SELECT name FROM users", int))):
    db = Plain()

def run() -> None:
    inner = DB()

    # error: [column-type-mismatch]
    @deco(inner.one("SELECT name FROM users", int))
    class Local(
        # error: [column-type-mismatch]
        base(inner.one("SELECT name FROM users", int)),
        # error: [column-type-mismatch]
        metaclass=deco(inner.one("SELECT name FROM users", int)),
    ):
        inner = Plain()
```

```python
from pkg.db import DB

class Plain:
    def one(self, query, row): ...

def deco(*args): ...
def base(*args): ...

db = Plain()

@deco(db.one("SELECT name FROM users", int))
class Hides(base(db.one("SELECT name FROM users", int)), metaclass=deco(db.one("SELECT name FROM users", int))):
    db = DB()

def run() -> None:
    inner = Plain()

    @deco(inner.one("SELECT name FROM users", int))
    class Local(base(inner.one("SELECT name FROM users", int))):
        inner = DB()

@deco(late.one("SELECT name FROM users", int))
class Unbound(base(late.one("SELECT name FROM users", int))):
    late = DB()

class Generic[DB](base(DB.one("SELECT name FROM users", int))): ...

# error:
```

A bound or a default of a type parameter is read in the scope around too, and so is a
decorator, which the type parameters do not reach. The header of a class in a class
reads the names of the class around it.

```python
from pkg.db import DB

class Plain:
    def one(self, query, row): ...

def deco(*args): ...
def base(*args): ...

db = DB()
T = DB()

# error: [column-type-mismatch]
class Bound[A: deco(db.one("SELECT name FROM users", int))]:
    db = Plain()

# error: [column-type-mismatch]
@deco(T.one("SELECT name FROM users", int))
class Decorated[T]:
    pass

class Outer:
    held = DB()

    # error: [column-type-mismatch]
    class Inner[B](base(held.one("SELECT name FROM users", int))):
        held = Plain()
```

## The decorators, defaults and annotations of a function

They are read in the scope around the function, where its parameters are not seen: `db`
is the `DB` of the module there, and `site` its `Website`.

```python
from pkg.db import DB, Website

db: DB
site: Website

# error: [column-type-mismatch] "int is expected but query column 'name' infers str"
@db.one("SELECT name FROM users", int)
def decorated(db: Website) -> None: ...

@site.one("SELECT name FROM users", int)
def other(site: DB) -> None: ...

def default(
    db: Website,
    site: DB,
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    row=db.one("SELECT name FROM users", int),
    none=site.one("SELECT name FROM users", int),
) -> None: ...

# error: [column-type-mismatch] "int is expected but query column 'name' infers str"
def typed[T: db.one("SELECT name FROM users", int)](db: Website) -> None: ...

# error: [column-type-mismatch] "int is expected but query column 'name' infers str"
def returns(db: Website) -> db.one("SELECT name FROM users", int): ...
```

## The types that annotations name

The annotation of a parameter is read where the function is defined, too: a name of it
is what the scope around makes it, whatever the body of the function binds it to later.

### A name the body binds

```python
from pkg.db import DB, Website

Conn = Website

def website(db: Conn) -> None:
    Conn = DB
    db.one("SELECT name FROM users", int)

def helper(db: DB) -> None:
    DB = Website
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    db.one("SELECT name FROM users", int)
```

### A name the body imports

```python
from pkg.db import DB, Website

Conn = Website

def website(db: Conn) -> None:
    from pkg.db import DB as Conn
    db.one("SELECT name FROM users", int)

def helper(db: DB) -> None:
    from pkg.db import Website as DB
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    db.one("SELECT name FROM users", int)
```

### An annotation in a string

```python
from pkg.db import DB, Website

Conn = Website

def website(db: "Conn") -> None:
    Conn = DB
    db.one("SELECT name FROM users", int)

def helper(db: "DB") -> None:
    DB = Website
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    db.one("SELECT name FROM users", int)
```

### A generic function

A type parameter of the function is the one name of an annotation the function has
itself: `T` is not the `DB` of the module.

```python
from pkg.db import DB, Website

Conn = Website
T = DB

def website[T](db: Conn, other: T) -> None:
    Conn = DB
    db.one("SELECT name FROM users", int)
    other.one("SELECT name FROM users", int)

def helper[T](db: DB, other: T) -> None:
    DB = Website
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    db.one("SELECT name FROM users", int)
    other.one("SELECT name FROM users", int)
```

### A method, a nested function, a parameter read after a test

```python
from pkg.db import DB, Website

class Service:
    def helper(self, db: DB) -> None:
        DB = Website
        # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
        db.one("SELECT name FROM users", int)

def outer() -> None:
    Conn = DB

    def website(db: Website | None) -> None:
        Website = DB
        if db is not None:
            db.one("SELECT name FROM users", int)

    def helper(db: Conn | None) -> None:
        Conn = Website
        if db is not None:
            # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
            db.one("SELECT name FROM users", int)

        def inner() -> None:
            # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
            db.one("SELECT name FROM users", int)
```

### An annotation in the body is the function's

```python
from pkg.db import DB, Website

def website() -> None:
    DB = Website
    db: DB = DB()
    # error:
    db.one("SELECT name FROM users", int)
```

### A name the body of the class binds

The annotation of a method is read in the body of its class, whose names are not
followed: `DB` there is not the `DB` of the module, whatever the method binds it to.

```python
from pkg.db import DB, Website

class Service:
    DB = Website

    def method(self, db: DB) -> None:
        DB = 1
        # error:
        db.one("SELECT name FROM users", int)
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
    # error: [column-type-mismatch]
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

## `isinstance` narrows

ty: `DB`.

```python
from pkg.db import DB

def run(x: object) -> None:
    if isinstance(x, DB):
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)
```

### In the branch only

The `else` branch and what follows the `if` hold what the name held before.

```python
from pkg.db import DB

def run(x: object) -> None:
    if isinstance(x, DB):
        pass
    else:
        x.one("SELECT name FROM users", int)
    x.one("SELECT name FROM users", int)

# error:
```

### After an early `return` or `raise`

ty: `DB` for both.

```python
from pkg.db import DB

def returned(x: object) -> None:
    if not isinstance(x, DB):
        return
    # error: [column-type-mismatch]
    x.one("SELECT name FROM users", int)

def raised(x: object) -> None:
    if not isinstance(x, DB):
        raise TypeError(x)
    # error: [column-type-mismatch]
    x.one("SELECT name FROM users", int)
```

### `assert`

```python
from pkg.db import DB

def run(x: object) -> None:
    assert isinstance(x, DB)
    # error: [column-type-mismatch]
    x.one("SELECT name FROM users", int)
```

### `and`

```python
from pkg.db import DB

def run(x: object, ready: bool) -> None:
    if ready and isinstance(x, DB):
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)
```

### A subclass stays what it is

ty: `Quiet`, whose `one` is not a helper.

```python
from pkg.db import DB

class Quiet(DB):
    def one(self, query, form):
        return None

def run(x: Quiet) -> None:
    if isinstance(x, DB):
        x.one("SELECT name FROM users", int)

# error:
```

### A binding after the test ends it

```python
from pkg.db import DB

def run(x: object, other: object) -> None:
    if isinstance(x, DB):
        x = other
        x.one("SELECT name FROM users", int)

# error:
```

### A class that is known

The test narrows a base class to its subclass, and leaves a subclass what it is. A name
known to be of an unrelated class would be of both classes, and a union of all its
members: neither is followed. ty: `DB`, `Cursor & DB`, `(DB & Cursor) | DB`.

```python
from pkg.db import DB, Cursor, Postgres

def narrowed(x: Postgres) -> None:
    if isinstance(x, DB):
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)

def unrelated(x: Cursor) -> None:
    if isinstance(x, DB):
        x.one("SELECT name FROM users", int)

def union(x: Cursor | DB) -> None:
    if isinstance(x, Postgres):
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)

def either(ready: bool, other) -> None:
    x = DB() if ready else other
    y = other
    if ready:
        y = Cursor()
    if isinstance(y, DB):
        y.one("SELECT name FROM users", int)
```

### A name tested twice, an annotation that is not followed

A second test of a name already narrowed states both classes. An annotation sqlty does
not follow (an alias of a union, a type variable) may state a class too, so only a name
without one, or annotated `object` or `Any`, takes the class of the test. ty: `DB`,
`DB`, `(Cursor & DB) | DB`, `T & DB`.

```python
from typing import Any
from pkg.db import DB, Cursor, Postgres

Either = Cursor | DB

def twice(x: object) -> None:
    if isinstance(x, Postgres):
        if isinstance(x, DB):
            x.one("SELECT name FROM users", int)

def then(x: object) -> None:
    assert isinstance(x, Postgres)
    if not isinstance(x, DB):
        return
    x.one("SELECT name FROM users", int)

def alias(x: Either) -> None:
    if isinstance(x, DB):
        x.one("SELECT name FROM users", int)

def variable[T: Cursor](x: T) -> None:
    if isinstance(x, DB):
        x.one("SELECT name FROM users", int)

def anything(x: Any, y) -> None:
    if isinstance(x, DB):
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)
    if isinstance(y, DB):
        # error: [column-type-mismatch]
        y.one("SELECT name FROM users", int)
```

### Many branches

Every branch that reaches the call counts, here ten that agree. A name more than 64
bindings reach at once is not followed.

```python
from pkg.db import DB, make_untyped

def run(kind: int) -> None:
    if kind == 1:
        db = DB()
    elif kind == 2:
        db = DB()
    elif kind == 3:
        db = DB()
    elif kind == 4:
        db = DB()
    elif kind == 5:
        db = DB()
    elif kind == 6:
        db = DB()
    elif kind == 7:
        db = DB()
    elif kind == 8:
        db = DB()
    elif kind == 9:
        db = DB()
    elif kind == 10:
        db = DB()
    else:
        db = DB()
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
    if kind == 11:
        db = make_untyped()
    db.one("SELECT name FROM users", int)
```

### Two tests, a binding in the test, another `isinstance`

```python
from pkg.db import DB, Postgres

def both(x: object) -> None:
    if isinstance(x, Postgres) and isinstance(x, DB):
        x.one("SELECT name FROM users", int)

def bound_in_the_test(x: object, other: object) -> None:
    if isinstance(x, DB) and (x := other):
        x.one("SELECT name FROM users", int)

def another(x: object, isinstance) -> None:
    if isinstance(x, DB):
        x.one("SELECT name FROM users", int)

# error:
```

### `elif`, loops, `try` and `match`

```python
from pkg.db import DB, Cursor

def chained(x: object) -> None:
    if isinstance(x, Cursor):
        pass
    elif isinstance(x, DB):
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)
    else:
        x.one("SELECT name FROM users", int)

def looped(things: list[object]) -> None:
    for x in things:
        if not isinstance(x, DB):
            continue
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)
    while things:
        x = things.pop()
        if not isinstance(x, DB):
            break
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)
    else:
        x.one("SELECT name FROM users", int)

def tried(x: object, other: object) -> None:
    assert isinstance(x, DB)
    try:
        x = other
    except ValueError:
        x.one("SELECT name FROM users", int)
    finally:
        x.one("SELECT name FROM users", int)
    x.one("SELECT name FROM users", int)

def kept(x: object) -> None:
    assert isinstance(x, DB)
    try:
        pass
    except ValueError:
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)
    else:
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)
    finally:
        # error: [column-type-mismatch]
        x.one("SELECT name FROM users", int)

def matched(x: object, command: object) -> None:
    assert isinstance(x, DB)
    match command:
        case "one":
            # error: [column-type-mismatch]
            x.one("SELECT name FROM users", int)
        case [x]:
            x.one("SELECT name FROM users", int)
    x.one("SELECT name FROM users", int)
```

### At the top of a module and in a class body

The statements of a module and of a class body run in order too. A function reads a
name of its module whenever it is called, so every binding of the module counts there.

```python
from pkg.db import DB, make_untyped

db = make_untyped()
db.one("SELECT name FROM users", int)
if isinstance(db, DB):
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
db = DB()
# error: [column-type-mismatch]
db.one("SELECT name FROM users", int)

def later() -> None:
    db.one("SELECT name FROM users", int)

class Report:
    source = make_untyped()
    source.one("SELECT name FROM users", int)
    source = DB()
    # error: [column-type-mismatch]
    source.one("SELECT name FROM users", int)
```

### What follows a `return` is not reached

```python
from pkg.db import DB

def run(db: DB) -> None:
    return
    db.one("SELECT name FROM users", int)

# error:
```

### A tuple of classes and `or` are not followed

ty: `DB | Cursor`, and `object`.

```python
from pkg.db import DB, Cursor

def run(x: object, ready: bool) -> None:
    if isinstance(x, (DB, Cursor)):
        x.one("SELECT name FROM users", int)
    if ready or isinstance(x, DB):
        x.one("SELECT name FROM users", int)

# error:
```

### A test against what is no class, and more classes than are followed

A test against a module, and one of a name that may hold a class, narrow nothing. The
same class by two bindings is that class. A name of more than 8 classes is not followed,
tested or not.

```python
from pkg import db as database
from pkg.db import DB, Cursor, make

shared = DB
shared = DB()

class A1(DB): ...
class A2(DB): ...
class A3(DB): ...
class A4(DB): ...
class A5(DB): ...
class A6(DB): ...
class A7(DB): ...
class A8(DB): ...
class A9(DB): ...

def a_module(db: object) -> None:
    if isinstance(db, database):
        db.one("SELECT name FROM users", int)

def a_class() -> None:
    db = shared
    if isinstance(db, DB):
        db.one("SELECT name FROM users", int)

def twice(flag: bool) -> None:
    if flag:
        db = DB()
    else:
        db = make()
    if isinstance(db, DB):
        # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
        db.one("SELECT name FROM users", int)

def nine(n: int) -> None:
    if n == 1:
        db = A1()
    elif n == 2:
        db = A2()
    elif n == 3:
        db = A3()
    elif n == 4:
        db = A4()
    elif n == 5:
        db = A5()
    elif n == 6:
        db = A6()
    elif n == 7:
        db = A7()
    elif n == 8:
        db = A8()
    else:
        db = A9()
    db.one("SELECT name FROM users", int)
    if isinstance(db, DB):
        db.one("SELECT name FROM users", int)
```

## Every binding counts

A name is followed only when every binding that reaches the call states its class. A
form that binds a name without stating one makes it unknown: the call is skipped, never
checked against what the name held before or what the module binds it to.

### An assignment expression

```python
from pkg.db import DB

def run(db: DB, other) -> None:
    if (db := other):
        db.one("SELECT name FROM users", int)

# error:
```

### A comprehension variable

```python
from pkg.db import DB

db = DB()

def run(others) -> None:
    [db.one("SELECT name FROM users", int) for db in others]
    {db: db.one("SELECT name FROM users", int) for db in others}
    list(db.one("SELECT name FROM users", int) for db in others)

# error:
```

### `except ... as`

```python
from pkg.db import DB

def run(db: DB) -> None:
    try:
        pass
    except Exception as db:
        db.one("SELECT name FROM users", int)

# error:
```

### An import in a function

An import binds its name in the function it is written in, and is followed there: the
name is what the function imports, not what the module binds it to.

```python
from pkg.db import DB

db = DB()

def run() -> None:
    import other as db
    db.one("SELECT name FROM users", int)

def run_from() -> None:
    from other import thing as db
    db.one("SELECT name FROM users", int)

# error:
```

```python
import other as DB

def run() -> None:
    from pkg.db import DB
    db = DB()
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)

def qualified() -> None:
    import pkg.db
    # error: [column-type-mismatch]
    pkg.db.DB().one("SELECT name FROM users", int)

class Holder:
    def __init__(self) -> None:
        from pkg.db import DB as Local
        self.db = Local()

    def run(self) -> None:
        # error: [column-type-mismatch]
        self.db.one("SELECT name FROM users", int)
```

### `global` at the top of a module

It changes nothing there.

```python
from pkg.db import DB

global db
db = DB()
# error: [column-type-mismatch]
db.one("SELECT name FROM users", int)
```

### `nonlocal`

```python
from pkg.db import DB

def run(other) -> None:
    db = DB()

    def swap() -> None:
        nonlocal db
        db = other

    swap()
    db.one("SELECT name FROM users", int)

# error:
```

### `global` in another function

```python
from pkg.db import DB

db = DB()

def swap(other) -> None:
    global db
    db = other

def run() -> None:
    db.one("SELECT name FROM users", int)

# error:
```

### `match ... as`

```python
from pkg.db import DB

def run(db: DB, other) -> None:
    match other:
        case [db]:
            db.one("SELECT name FROM users", int)
        case {"db": _, **db}:
            db.one("SELECT name FROM users", int)
        case object() as db:
            db.one("SELECT name FROM users", int)

# error:
```

### A lambda parameter

```python
from pkg.db import DB

db = DB()

run = lambda db: db.one("SELECT name FROM users", int)

# error:
```

### A class in a function

The class is another one than the helper's, whatever its name.

```python
from pkg.db import DB

def run() -> None:
    class DB:
        def one(self, query, form): ...

    db = DB()
    db.one("SELECT name FROM users", int)

# error:
```

### A function in a function

```python
from pkg.db import make

def run() -> None:
    def make():
        return object()

    make().one("SELECT name FROM users", int)

# error:
```

### A parameter assigned again

ty: `object`.

```python
from pkg.db import DB

def run(db: DB) -> None:
    db = object()
    db.one("SELECT name FROM users", int)

# error:
```

### A class in a class

`Outer.DB` is another class than `DB`.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class DB:
    def one(self, query: Annotated[LiteralString, SQL], form: type, /): ...

class Outer:
    class DB:
        def one(self, query, form): ...

        def run(self) -> None:
            self.one("SELECT name FROM users", int)

def run() -> None:
    Outer.DB().one("SELECT name FROM users", int)

# error:
```

### A class in a class or in a function declares its own helpers

Of a class defined in another class or in a function, the helpers it declares itself
are known: on `self` in its methods, and on the name where it is defined.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Outer:
    class Query:
        def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

        def first(self) -> None:
            # error: [column-type-mismatch]
            self.one("SELECT name FROM users", int)

def run() -> None:
    class Local:
        def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

        def first(self) -> None:
            # error: [column-type-mismatch]
            self.one("SELECT name FROM users", int)

    made = Local()
    # error: [column-type-mismatch]
    made.one("SELECT name FROM users", int)
```

A name the scope around the class binds again may be another class by the time it is
used, and two classes of one name are not told apart: both are skipped.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def make(): ...

def rebound() -> None:
    class Local:
        def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

        def first(self) -> None:
            self.one("SELECT name FROM users", int)

    Local = make()
    made = Local()
    made.one("SELECT name FROM users", int)

def one_of_two() -> None:
    class Twice:
        def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

    made = Twice()
    made.one("SELECT name FROM users", int)

def other_of_two() -> None:
    class Twice:
        def one(self, query, form): ...

        def first(self) -> None:
            self.one("SELECT name FROM users", int)

def shadowed(Hidden) -> None:
    class Hidden:
        def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

    made = Hidden()
    made.one("SELECT name FROM users", int)

# error:
```

### A lambda and a generator run later

The body runs when it is called or iterated, so the name is whatever the function
binds it to by then, as in a nested `def`. ty: `DB | object`.

```python
from pkg.db import DB

def run() -> None:
    db = DB()
    later = lambda: db.one("SELECT name FROM users", int)
    each = (db.one("SELECT name FROM users", int) for _ in range(2))
    db = object()
    later()
    list(each)

def top() -> None:
    db = DB()
    later = lambda: db.one("SELECT name FROM users", int)
    db = object()

def looped(items: list[int]) -> None:
    for item in items:
        db = object()
        db = DB()
        later = lambda: db.one("SELECT name FROM users", int)

def wrapped() -> None:
    db = DB()
    db = wrap(lambda: db.one("SELECT name FROM users", int))

# error:
```

At the top of a module too:

```python
from pkg.db import DB

db = DB()
later = lambda: db.one("SELECT name FROM users", int)
db = object()

# error:
```

One whose bindings agree is checked, and a list comprehension runs where it is written:

```python
from pkg.db import DB

def run() -> None:
    db = DB()
    # error: [column-type-mismatch]
    later = lambda: db.one("SELECT name FROM users", int)

def now() -> None:
    db = DB()
    # error: [column-type-mismatch]
    found = [db.one("SELECT name FROM users", int) for _ in range(2)]
    db = object()
```

So is one the function does not bind the name after, which keeps what a test says of it:

```python
from pkg.db import DB

def run(x: object) -> None:
    if isinstance(x, DB):
        # error: [column-type-mismatch]
        later = lambda: x.one("SELECT name FROM users", int)
        # error: [column-type-mismatch]
        every = all(x.one("SELECT name FROM users", int) for _ in range(2))
```

### A name the class body binds

An attribute's value and annotation are read in the body of the class, where `DB` is
what the body binds it to. ty: `object`.

```python
from pkg.db import DB

class Aliased:
    DB = object
    db = DB()

class Annotated:
    DB = object
    db: DB

class Nested:
    class DB:
        def one(self, query, form): ...

    db = DB()

    def make(self) -> DB: ...

class Parameters:
    DB = object

    def __init__(self, db: DB) -> None:
        self.db = db

    def run(self, other: DB) -> None:
        other.one("SELECT name FROM users", int)

def run(a: Aliased, b: Annotated, c: Nested, d: Parameters) -> None:
    a.db.one("SELECT name FROM users", int)
    b.db.one("SELECT name FROM users", int)
    c.db.one("SELECT name FROM users", int)
    c.make().one("SELECT name FROM users", int)
    d.db.one("SELECT name FROM users", int)

# error:
```

An annotation alone declares the attribute and binds nothing, so the names beside it
are still the module's:

```python
from pkg import db
from pkg.db import DB

class Site:
    db: db.DB
    DB: DB
    other: DB

class Valued:
    DB: DB = DB()

def run(site: Site, valued: Valued) -> None:
    # error: [column-type-mismatch]
    site.db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    site.DB.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    site.other.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    valued.DB.one("SELECT name FROM users", int)
```

A name the body binds once, to the module's name, is the module's:

```python
from pkg.db import DB

shared = DB()

class Holder:
    shared = shared

def run(holder: Holder) -> None:
    # error: [column-type-mismatch]
    holder.shared.one("SELECT name FROM users", int)
```

### `del`, a loop variable and a conditional value

```python
from pkg.db import DB

def deleted(db: DB) -> None:
    del db
    db.one("SELECT name FROM users", int)

def looped(db: DB, others) -> None:
    for db in others:
        db.one("SELECT name FROM users", int)

def either(ready: bool) -> None:
    db = DB() if ready else None
    db.one("SELECT name FROM users", int)

# error:
```

### A branch that does not reach the call

A branch that returns binds nothing the call sees. ty: `DB`.

```python
from pkg.db import DB

def run(db: DB, other) -> None:
    if other:
        db = other
        return
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
```

### Branches that disagree

The name is of either class, and both reach a helper that takes the call the same way
(see [A union of classes](#a-union-of-classes)). ty: `DB | Cursor`.

```python
from pkg.db import DB, Cursor

def run(ready: bool) -> None:
    if ready:
        db = DB()
    else:
        db = Cursor()
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
```

Classes whose helpers take the call another way leave it unchecked. ty: `DB | Reader`.

```python
from pkg.db import DB
from pkg.reader import Reader

def run(ready: bool) -> None:
    if ready:
        db = DB()
    else:
        db = Reader()
    db.one("SELECT name FROM users", int)

# error:
```

### A `break` goes past the `else` of its loop

The `else` of a loop runs when the loop ends without a `break`: what follows the loop is
reached through the `else` and from each `break`. ty: `Unknown | DB` for the first two
functions, `object` for the third.

```python
from pkg.db import DB

def run(rows, make) -> None:
    db = make()
    for row in rows:
        if row:
            break
    else:
        db = DB()
    db.one("SELECT name FROM users", int)

def poll(rows, make) -> None:
    db = make()
    while rows:
        if rows.pop():
            break
    else:
        db = DB()
    db.one("SELECT name FROM users", int)

def tested(rows, db: object) -> None:
    for row in rows:
        if row:
            break
    else:
        assert isinstance(db, DB)
    db.one("SELECT name FROM users", int)

# error:
```

An `else` that returns leaves the `break` as the one way on. ty: `DB`.

```python
from pkg.db import DB

def run(rows, db: DB) -> None:
    for row in rows:
        if row:
            break
    else:
        return
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)

def poll(rows, db: DB) -> None:
    while True:
        if rows:
            break
    else:
        raise LookupError
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
```

A loop without a `break` goes on through its `else` only. ty: `DB`, then nothing follows.

```python
from pkg.db import DB

def run(rows, make) -> None:
    db = make()
    for row in rows:
        pass
    else:
        db = DB()
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)

def ended(rows, db: DB) -> None:
    for row in rows:
        continue
    else:
        return
    db.one("SELECT name FROM users", int)
```

### A `with` body that is cut short

A context manager may swallow an exception (`contextlib.suppress`), so the body of a
`with` may end after any of its statements, and what follows it runs even when the
body returns. ty: `DB | Other` for the first; it follows `suppress` only.

```python
import contextlib
from pkg.db import DB

class Other:
    def one(self, query: str, form: type, /) -> None: ...

def rebound(text: str) -> None:
    db = Other()
    with contextlib.suppress(ValueError):
        db = DB.connect(int(text))
    db.one("SELECT name FROM users", int)

def narrowed(db: object, text: str) -> None:
    if not isinstance(db, DB):
        with contextlib.suppress(ValueError):
            int(text)
            return
    db.one("SELECT name FROM users", int)

def kept(db: DB, lock) -> None:
    with lock:
        other = DB()
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    other.one("SELECT name FROM users", int)
```

### An import with a fallback

The two imports bind the name to different classes: the call is checked when both reach
a helper that takes it the same way. ty: `<class 'DB'> | <class 'Postgres'>` for
`Client`, and `Postgres` for `db`, as `DB` is a subclass of it.

```python
try:
    from pkg.db import DB as Client
except ImportError:
    from pkg.db import Postgres as Client

def run(db: Client) -> None:
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
```

An import under a condition is read the same way.

```python
import sys

if sys.platform == "win32":
    from pkg.db import Cursor as Client
else:
    from pkg.db import DB as Client

def run(db: Client) -> None:
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    Client().one("SELECT name FROM users", int)
```

Two helpers that take the call another way, or a fallback that is no class, leave it
unchecked.

```python
try:
    from pkg.db import DB as Client
except ImportError:
    from pkg.reader import Reader as Client

try:
    from pkg.db import DB as Maybe
except ImportError:
    Maybe = object

def run(db: Client, other: Maybe) -> None:
    db.one("SELECT name FROM users", int)
    other.one("SELECT name FROM users", int)

# error:
```

The same class from both is that class.

```python
try:
    from pkg.db import DB as Client
except ImportError:
    from pkg.db import DB as Client

def run(db: Client) -> None:
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
```

### A type parameter

A type parameter of a function or of a class is a name of its own, not the class the
module binds the name to. ty: `DB@run`, `DB@Box`, then `DB`.

```python
from pkg.db import DB

def run[DB](db: DB) -> None:
    db.one("SELECT name FROM users", int)

class Box[DB]:
    item: DB

    def first(self, db: DB) -> None:
        db.one("SELECT name FROM users", int)

    def second(self) -> None:
        self.item.one("SELECT name FROM users", int)

def plain(db: DB) -> None:
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
```

A type parameter of a class is seen in its methods, and in the functions in them.

```python
from pkg.db import DB, make_untyped

class Box[DB]:
    def first(self) -> None:
        db: DB = make_untyped()
        db.one("SELECT name FROM users", int)

        def inner(other: DB) -> None:
            other.one("SELECT name FROM users", int)

# error:
```

### A name the module declares, read in a class

A name of the module is what the module declares it to be, whatever the class it is
read in. `Self` at the top of a module names no class, in a class body or outside one.

```python
from typing import Self
from pkg.db import DB, make_untyped

held: Self = make_untyped()
db: DB = make_untyped()

class Sub(DB):
    class Inner:
        mine = held
        mine.one("SELECT name FROM users", int)
        held.one("SELECT name FROM users", int)
        # error: [column-type-mismatch]
        db.one("SELECT name FROM users", int)

held.one("SELECT name FROM users", int)
# error: [column-type-mismatch]
db.one("SELECT name FROM users", int)
```

### More bindings than classes

A name bound in more than 8 ways is followed while the bindings give no more than 8
classes: here one.

```python
from typing import cast
from pkg import db as database
from pkg.db import DB, Website, make

def m1() -> DB: ...
def m2() -> DB: ...
def m3() -> DB: ...
def m4() -> DB: ...
def m5() -> DB: ...
def m6() -> DB: ...
def m7() -> DB: ...

def calls(n: int) -> None:
    if n == 1:
        db = m1()
    elif n == 2:
        db = m2()
    elif n == 3:
        db = m3()
    elif n == 4:
        db = m4()
    elif n == 5:
        db = m5()
    elif n == 6:
        db = m6()
    elif n == 7:
        db = m7()
    elif n == 8:
        db = make()
    else:
        db = DB()
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    db.one("SELECT name FROM users", int)

def mixed(n: int, site: Website) -> None:
    if n == 1:
        db = site.db
    elif n == 2:
        db = cast(DB, n)
    elif n == 3:
        db: DB = n
    elif n == 4:
        db = m1()
    elif n == 5:
        db = m2()
    elif n == 6:
        db = m3()
    elif n == 7:
        db = database.make()
    elif n == 8:
        db = make()
    else:
        db = DB()
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    db.one("SELECT name FROM users", int)
```

### A parameter annotated with a name the class body binds

The annotation is read in the body of the class, which is not followed: the parameter
is unknown, in a function of the method and with another binding too.

```python
from pkg.db import DB, Cursor, make

class Holder:
    DB = Cursor

    def nested(self, db: DB) -> None:
        def inner() -> None:
            db.one("SELECT name FROM users", int)

        inner()

    def rebound(self, db: DB, flag: bool) -> None:
        if flag:
            db = make()
        db.one("SELECT name FROM users", int)

# error:
```

### Both branches end

```python
from pkg.db import DB, make

def run(flag: bool) -> None:
    if flag:
        db = DB()
        return
    else:
        db = make()
        raise ValueError
    db.one("SELECT name FROM users", int)

# error:
```

### A name one statement binds twice

The bindings count in the order they take effect: `as` binds after `:=`.

```python
from pkg.db import Transaction

def run() -> None:
    with (cursor := Transaction()) as cursor:
        # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
        cursor.one("SELECT name FROM users", int)
```

### A comprehension that unpacks, or binds no name

```python
from pkg.db import DB

def run(db: DB, rows: list) -> None:
    [db.one("SELECT name FROM users", int) for *db, _ in rows]
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    [db.one("SELECT name FROM users", int) for rows[0] in rows]
```

### A parameter specification and a variadic type parameter

They are type parameters as any other.

```python
from pkg.db import DB

class Ts(DB): ...
class P(DB): ...

def spread[*Ts, **P](db: DB) -> Ts: ...
def spec[*Ts, **P](db: DB) -> P: ...
def plain(db: DB) -> Ts: ...

def run() -> None:
    spread(DB()).one("SELECT name FROM users", int)
    spec(DB()).one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    plain(DB()).one("SELECT name FROM users", int)
```

## A diamond follows the method resolution order

`Both` inherits `one` from `Right`: Python looks in `Left`, `Right`, then `Base`. ty:
`bound method Both.one` of `Right`.

```python
from pkg.db import DB

class Left(DB):
    pass

class Right(DB):
    def one(self, query, form):
        return None

class Both(Left, Right):
    pass

def run(both: Both) -> None:
    both.one("SELECT name FROM users", int)

# error:
```

### The helper of the first base

```python
from pkg.db import DB

class Left(DB):
    pass

class Right(DB):
    pass

class Both(Left, Right):
    pass

def run(both: Both) -> None:
    # error: [column-type-mismatch]
    both.one("SELECT name FROM users", int)
```

### Bases in no consistent order

Python raises `TypeError` for the class; the call is skipped.

```python
from pkg.db import DB

class Left(DB):
    pass

class Wrong(DB, Left):
    pass

def run(wrong: Wrong) -> None:
    wrong.one("SELECT name FROM users", int)

# error:
```

A class with such bases still has the helpers it declares itself.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class A: ...
class B: ...
class Left(A, B): ...
class Right(B, A): ...

class Wrong(Left, Right):
    def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

def run(wrong: Wrong) -> None:
    # error: [column-type-mismatch]
    wrong.one("SELECT name FROM users", int)
```

### Bases that are not followed

A base that is not found, or one the standard library defines, is not read: each is a
class of its own, so two of them are not the same base twice. The helpers the class
declares itself are found, on `self` too; a method it does not declare may be one of
such a base, and is skipped.

```python
from typing import Annotated, LiteralString
from sqlty import SQL
from missing.models import db, other

class Store(dict, object):
    def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

    def first(self) -> None:
        # error: [column-type-mismatch]
        self.one("SELECT name FROM users", int)

class Repo(db.Model, other.Base):
    @staticmethod
    def stat[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

class Failed(Exception, dict):
    def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

def run(store: Store, repo: Repo) -> None:
    # error: [column-type-mismatch]
    store.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    Store().one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    Repo.stat("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    Failed().one("SELECT name FROM users", int)
    repo.get("SELECT name FROM users", int)
```

Two classes that name the same base share it: `Second` comes before `dict` for
`Both`, and its helper is found. A base that is computed is another class wherever it
is written, and may define the method before `Second` does.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def make_one(): ...
def make_two(): ...

class First(dict): ...

class Second(dict):
    held: "Second"

    def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

class Both(First, Second): ...

class Made(make_one()): ...

class Other(make_two()):
    held: "Other"

    def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

class Either(Made, Other): ...

def run(both: Both, either: Either) -> None:
    # error: [column-type-mismatch]
    both.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    both.held.one("SELECT name FROM users", int)
    either.one("SELECT name FROM users", int)
    either.held.one("SELECT name FROM users", int)
    Either().one("SELECT name FROM users", int)
```

A name the module binds twice may be another class at each class statement: the two
bases are not shared.

```python
from typing import Annotated, LiteralString
from sqlty import SQL
from missing import models, other

Base = models.Model

class First(Base): ...

Base = other.Model

class Second(Base):
    def one[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

class Both(First, Second): ...

def run(both: Both, second: Second) -> None:
    both.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    second.one("SELECT name FROM users", int)
```

### A base written twice, a base with type arguments

A base written with type arguments is the class. Python refuses a class with the same
base twice, which has no order. A base that is not followed is one class by the dotted
name it is written as, here `database.Missing`: `Same` has it once, after `DB`.

```python
from typing import Generic, TypeVar
from pkg import db as database
from pkg.db import DB

T = TypeVar("T")

class Box(DB, Generic[T]): ...
class Wrapped(Box[int]): ...
class Twice(DB, DB): ...
class Odd(DB, database.Missing): ...
class Same(Odd, database.Missing): ...

def run(box: Box[int], wrapped: Wrapped, twice: Twice, odd: Odd, same: Same) -> None:
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    box.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    wrapped.one("SELECT name FROM users", int)
    twice.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    odd.one("SELECT name FROM users", int)
    # error: [column-type-mismatch] "int is expected but query column 'name' infers str"
    same.one("SELECT name FROM users", int)
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

## A registered method of a class its module gets from elsewhere

A library that is found may still not say what its class is: it takes it from a C
extension, by a star import, by an import with a fallback or by `__getattr__`. A method
registered as `module.Class.method` is then found on the class by that path, as when the
library is not found at all.

```toml
[[tool.sqlty.functions]]
import-path = "ext.Db.fetch"
query-param = "sql"

[[tool.sqlty.functions]]
import-path = "star.Db.fetch"
query-param = "sql"

[[tool.sqlty.functions]]
import-path = "either.Db.fetch"
query-param = "sql"

[[tool.sqlty.functions]]
import-path = "lazy.Db.fetch"
query-param = "sql"
```

`ext/__init__.py`:

```python
from ext._impl import Db
```

`star/__init__.py`:

```python
from star.core import *
```

`star/core.py`:

```python
class Db:
    def fetch(self, sql): ...
```

`either/__init__.py`:

```python
try:
    from either._fast import Db
except ImportError:
    from either._slow import Db
```

`either/_slow.py`:

```python
class Db:
    def fetch(self, sql): ...
```

`lazy/__init__.py`:

```python
def __getattr__(name): ...
```

### Re-exported from a module that is not found

```python
import ext
from ext import Db

def run(db: Db, other: ext.Db) -> None:
    # error: [unknown-column]
    db.fetch(sql="SELECT nope FROM users")
    # error: [unknown-column]
    other.fetch(sql="SELECT nope FROM users")
```

### A star import

```python
import star
from star import Db

def run(db: Db, other: star.Db) -> None:
    # error: [unknown-column]
    db.fetch(sql="SELECT nope FROM users")
    # error: [unknown-column]
    other.fetch(sql="SELECT nope FROM users")
```

### An import with a fallback, and `__getattr__`

```python
import lazy
from either import Db

def run(db: Db, other: lazy.Db) -> None:
    # error: [unknown-column]
    db.fetch(sql="SELECT nope FROM users")
    # error: [unknown-column]
    other.fetch(sql="SELECT nope FROM users")
```

### A class of the module with the name of the imported one

A class the module defines is that class, whatever the name was imported as before: its
own method is not the registered one. Where the name may be either, the call is not
checked.

```python
from star import Db

class Db:
    def fetch(self, sql): ...

def run(db: Db) -> None:
    # error:
    db.fetch(sql="SELECT nope FROM users")
    # error:
    Db().fetch(sql="SELECT nope FROM users")
```

```python
try:
    from star import Db
except ImportError:
    class Db:
        def fetch(self, sql): ...

def run(db: Db) -> None:
    # error:
    db.fetch(sql="SELECT nope FROM users")
```

### A class in a class

A class defined in another class is not followed, so a method registered on it is found
by its path, on a class that is read and on one that is not.

```toml
[[tool.sqlty.functions]]
import-path = "nested.Outer.Inner.fetch"
query-param = "sql"

[[tool.sqlty.functions]]
import-path = "ext.Outer.Inner.fetch"
query-param = "sql"
```

`nested.py`:

```python
class Outer:
    class Inner:
        def fetch(self, sql): ...
```

```python
import ext
import nested
from nested import Outer

def run(one: Outer.Inner, two: nested.Outer.Inner, three: ext.Outer.Inner) -> None:
    # error: [unknown-column]
    one.fetch(sql="SELECT nope FROM users")
    # error: [unknown-column]
    two.fetch(sql="SELECT nope FROM users")
    # error: [unknown-column]
    three.fetch(sql="SELECT nope FROM users")
```

### A name assigned the class

`Db = Other` names the class `Other`: the method registered as `aliased.Db.fetch` is the
one `Other` defines, with its parameters, so the query is found by position too.

```toml
[[tool.sqlty.functions]]
import-path = "aliased.Db.fetch"
query-param = "sql"
```

`aliased.py`:

```python
class Other:
    def fetch(self, sql: str) -> None: ...

Db = Other
```

```python
from aliased import Db, Other

def run(db: Db, other: Other) -> None:
    # error: [unknown-column]
    db.fetch("SELECT nope FROM users")
    # error: [unknown-column]
    other.fetch("SELECT nope FROM users")
```

### A name assigned a class of a name bound twice

A name bound twice is not read for the parameters: the query is found by name only, and
the second class of that name is not the registered one.

```toml
[[tool.sqlty.functions]]
import-path = "rebound.Db.fetch"
query-param = "sql"
```

`rebound.py`:

```python
class Other:
    def fetch(self, sql: str) -> None: ...

Db = Other

class Other:
    def fetch(self, first: str, sql: str) -> None: ...
```

```python
from rebound import Db

def run(db: Db) -> None:
    # error: [unknown-column]
    db.fetch(sql="SELECT nope FROM users")
```

```python
from rebound import Db, Other

def run(db: Db, other: Other) -> None:
    db.fetch("SELECT nope FROM users")
    other.fetch("first", "SELECT nope FROM users")

# error:
```

### A name assigned a name assigned the class

It is followed as a name assigned the class is.

```toml
[[tool.sqlty.functions]]
import-path = "chained.Db.fetch"
query-param = "sql"
```

`chained.py`:

```python
class Other:
    def fetch(self, sql: str) -> None: ...

Mid = Other
Db = Mid
```

```python
from chained import Db

def run(db: Db) -> None:
    # error: [unknown-column]
    db.fetch("SELECT nope FROM users")
```

### An attribute of a class assigned another class

```toml
[[tool.sqlty.functions]]
import-path = "shadow.Outer.Inner.fetch"
query-param = "sql"
```

`shadow.py`:

```python
class Real:
    def fetch(self, sql): ...

class Outer:
    Inner = Real
```

```python
import shadow
from shadow import Outer

def run(one: Outer.Inner, two: shadow.Outer.Inner) -> None:
    # error: [unknown-column]
    one.fetch(sql="SELECT nope FROM users")
    # error: [unknown-column]
    two.fetch(sql="SELECT nope FROM users")
```

### A name that is not registered stays unknown

```python
from ext import Other

def run(db: Other) -> None:
    db.fetch(sql="SELECT nope FROM users")

# error:
```

### A marked method of a class imported with a fallback

The class is known by the path it is registered by. A method its definition marks is
found through the names the module imports.

```toml
[[tool.sqlty.functions]]
import-path = "shim.Db.fetch"
query-param = "sql"
```

`shim/__init__.py`:

```python
try:
    from shim._fast import Db
except ImportError:
    from shim._slow import Db
```

`shim/_slow.py`:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def fetch(self, sql): ...
    def one(self, sql: Annotated[LiteralString, SQL]): ...
```

```python
from shim import Db

def run(db: Db) -> None:
    # error: [unknown-column] "column `nope` does not exist in `users`"
    db.fetch(sql="SELECT nope FROM users")
    # error: [unknown-column] "column `nope` does not exist in `users`"
    db.one("SELECT nope FROM users")
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

### A stub that names the stub of another library

A framework's stub declares an attribute of a class of a driver's stub. The checked code
names the framework only: the driver's stub is read because the framework's names it.

`typings/framework/__init__.pyi`:

```python
from lib import Postgres

class Site:
    db: Postgres

def connect() -> Postgres: ...
```

```python
from framework import Site, connect

def run(site: Site) -> None:
    # error: [column-type-mismatch]
    site.db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    connect().one("SELECT name FROM users", int)
```

### A stub that declares the class without the method

The stub is what the library declares: the parameters of a method registered on a class
the stub declares without it are not looked for in the library, so only a query passed by
name is checked.

```toml
[[tool.sqlty.functions]]
import-path = "partial.Db.fetch"
query-param = "sql"
```

`typings/partial/__init__.pyi`:

```python
class Db: ...
```

`partial/__init__.py`:

```python
class Db:
    def fetch(self, sql): ...
```

```python
from partial import Db

def run(db: Db) -> None:
    # error: [unknown-column] "column `nope` does not exist in `users`"
    db.fetch(sql="SELECT nope FROM users")
    db.fetch("SELECT nope FROM users")
```

## `stub-paths`

The directories are read in order: the first that describes a module gives its stub.
`first-stubs` describes `first`, and a package is taken before a module of its name. A
`.pyi` under no directory of `stub-paths` is not read.

```toml
[tool.sqlty]
schema = "schema.sql"
stub-paths = ["stubs", "more"]
```

`stubs/first-stubs/__init__.pyi`:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def one[T](self, sql: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...
```

`more/first/__init__.pyi`:

```python
class Db:
    def one(self, sql, form): ...
```

`stubs/both/__init__.pyi`:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def one[T](self, sql: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...
```

`stubs/both.pyi`:

```python
class Db:
    def one(self, sql, form): ...
```

`typings/third/__init__.pyi`:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def one[T](self, sql: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...
```

```python
from first import Db
from both import Db as Both
from third import Db as Third

def run(db: Db, both: Both, third: Third) -> None:
    # error: [column-type-mismatch]
    db.one("SELECT name FROM users", int)
    # error: [column-type-mismatch]
    both.one("SELECT name FROM users", int)
    third.one("SELECT name FROM users", int)
```
