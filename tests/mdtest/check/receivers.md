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

ty: `DB | Cursor`.

```python
from pkg.db import DB, Cursor

def run(ready: bool) -> None:
    if ready:
        db = DB()
    else:
        db = Cursor()
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

### An import with a fallback

The two imports bind the name to different classes, so the call is skipped, even when
both classes have the helper. ty: `<class 'DB'> | <class 'Postgres'>` for `Client`, and
`Postgres` for `db`, as `DB` is a subclass of it.

```python
try:
    from pkg.db import DB as Client
except ImportError:
    from pkg.db import Postgres as Client

def run(db: Client) -> None:
    db.one("SELECT name FROM users", int)

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
