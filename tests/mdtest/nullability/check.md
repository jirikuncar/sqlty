# Nullability: lint checks against row types

`nullable-mismatch` reports a row field that is not optional for a column that can be
NULL; `redundant-optional` (opt-in) reports an optional field for a column that cannot be.

## A non-optional field for a nullable column

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, nick TEXT NOT NULL);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    nick: str

# error: [nullable-mismatch] "User.name is str but query column 'name' can be NULL"
q = SQL[User]("--sql\nSELECT id, name, nick FROM users")
```

## Optional fields are accepted

`T | None` and `Optional[T]` fit a nullable column, and also a column that cannot be
NULL: a wider annotation is not an error, only `redundant-optional` reports it.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, nick TEXT NOT NULL);
```

```python
from typing import Optional, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int | None
    name: Optional[str]
    nick: str | None

q = SQL[User]("--sql\nSELECT id, name, nick FROM users")
# error:
```

## Dataclasses, NamedTuples and Pydantic models

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class User:
    id: int
    name: str

# error: [nullable-mismatch] "User.name is str but query column 'name' can be NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

```python
from typing import NamedTuple
from sqlty import SQL

class Row(NamedTuple):
    id: int
    name: str

# error: [nullable-mismatch] "Row.name is str but query column 'name' can be NULL"
q = SQL[Row]("--sql\nSELECT id, name FROM users")
```

```python
from pydantic import BaseModel
from sqlty import SQL

class Model(BaseModel):
    id: int
    name: str

# error: [nullable-mismatch] "Model.name is str but query column 'name' can be NULL"
q = SQL[Model]("--sql\nSELECT id, name FROM users")
```

## Outer joins

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
```

```python
from typing import TypedDict
from sqlty import SQL

class UserPost(TypedDict):
    name: str
    title: str

# error: [nullable-mismatch] "UserPost.title is str but query column 'title' can be NULL"
q = SQL[UserPost]("--sql\nSELECT u.name, p.title FROM users u LEFT JOIN posts p ON p.user_id = u.id")
```

## Unknown nullability is never reported

Casts with no schema carry no nullability, so they are neither optional nor reported.

```toml
[tool.sqlty]

[tool.sqlty.rules]
redundant-optional = "error"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str | None

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Fields typed Any are never reported

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-optional = "error"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import Any, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: Any
    name: Any

q = SQL[User]("--sql\nSELECT id, name FROM users")
# error:
```

## redundant-optional

Strict mode: enable the rule to flag `| None` on columns that cannot be NULL. It stays
off unless enabled.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-optional = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import Optional, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: Optional[int]
    name: str | None

# error: [redundant-optional] "User.id is int | None but query column 'id' is never NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Switching nullable-mismatch off

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
nullable-mismatch = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql\nSELECT id, name FROM users")
# error:
```

## Lowering the severity of nullable-mismatch

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
nullable-mismatch = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# error: [nullable-mismatch] "User.name is str but query column 'name' can be NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Helper row types

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch[T](query: Annotated[LiteralString, SQL], row: type[T]) -> list[T]:
    raise NotImplementedError


# error: [nullable-mismatch] "tuple[int, str] item 1 is str but query column 'name' can be NULL"
rows = fetch("SELECT id, name FROM users", tuple[int, str])
rows = fetch("SELECT id, name FROM users", tuple[int, str | None])
```

## A row type shared by queries

`redundant-optional` reports a field only when no query that loads the row type can
return NULL for it.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-optional = "error"
```

`schema.sql`:

```sql
CREATE TABLE coupons (id INTEGER PRIMARY KEY, owner_id INTEGER);
```

```python
from typing import Annotated, LiteralString, TypedDict
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

class Coupon(TypedDict):
    id: int
    owner_id: int | None

class Mine(TypedDict):
    id: int
    owner_id: int | None

mine = fetch_all("SELECT id, owner_id FROM coupons WHERE owner_id = %(owner)s", Coupon)
any_ = fetch_all("SELECT id, owner_id FROM coupons WHERE owner_id = %(owner)s OR owner_id IS NULL", Coupon)
# error: [redundant-optional] "Mine.owner_id is int | None but query column 'owner_id' is never NULL"
only = fetch_all("SELECT id, owner_id FROM coupons WHERE owner_id = %(owner)s", Mine)
```

## Parameters follow the argument at the call site

A placeholder echoed into the result can be NULL, since a caller may bind `None`.
A helper call that passes a value whose type excludes `None` makes it non-null
for that call; `None`, an optional or an unknown argument keeps it nullable.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from typing import Annotated, LiteralString, TypedDict
from sqlty import SQL

def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /, **params: object) -> T: ...

class Doc(TypedDict):
    id: int
    key: str

class Row(TypedDict):
    key: str
    doc: Doc

def lookup(maybe: str | None) -> None:
    ok = fetch_one("SELECT %(key)s::TEXT AS key, jsonb_build_object('id', u.id, 'key', %(key)s::TEXT) AS doc FROM users u", Row, key="x")
    # error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc.key can be null but is declared str"
    # error: [nullable-mismatch] "Row.key is str but query column 'key' can be NULL"
    none = fetch_one("SELECT %(key)s::TEXT AS key, jsonb_build_object('id', u.id, 'key', %(key)s::TEXT) AS doc FROM users u", Row, key=None)
    # error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc.key can be null but is declared str"
    # error: [nullable-mismatch] "Row.key is str but query column 'key' can be NULL"
    optional = fetch_one("SELECT %(key)s::TEXT AS key, jsonb_build_object('id', u.id, 'key', %(key)s::TEXT) AS doc FROM users u", Row, key=maybe)
```

### From `P` of `SQL[R, P]` and the driver call

`P` of `SQL[R, P]` states what the query is bound to, and so do the arguments of the
driver call that runs a `SQL[R]` query.

```python
from typing import NamedTuple, TypedDict

import psycopg

from sqlty import SQL

class Row(NamedTuple):
    key: str

class Key(TypedDict):
    key: str

class MaybeKey(TypedDict):
    key: str | None

a = SQL[Row, Key]("--sql\nSELECT %(key)s::TEXT AS key FROM users")
b = SQL[Row, tuple[str]]("--sql\nSELECT %s::TEXT AS key FROM users")
# error: [nullable-mismatch] "Row.key is str but query column 'key' can be NULL"
c = SQL[Row, MaybeKey]("--sql\nSELECT %(key)s::TEXT AS key FROM users")

def run(conn: psycopg.Connection, key: str, maybe: str | None) -> None:
    conn.execute(SQL[Row]("--sql\nSELECT %(key)s::TEXT AS key FROM users"), {"key": key})
    # error: [nullable-mismatch] "Row.key is str but query column 'key' can be NULL"
    conn.execute(SQL[Row]("--sql\nSELECT %(key)s::TEXT AS key FROM users"), {"key": maybe})
```

### Constants

A name bound once to a literal, with or without `Final`, has the literal's type, and so
does a constant imported from a checked module, also one bound to another constant. A
call result and a parameter of unknown type keep the placeholder nullable.

```toml
[[tool.sqlty.functions]]
import-path = "db.fetch_all"
query-param = "query"
return-type-param = "row"
```

`db.py`:

```python
from typing import Any

def fetch_all[T](query: str, row: type[T], /, **params: Any) -> list[T]: ...
```

`limits.py`:

```python
LIMIT = "x"
ALIAS = LIMIT
```

```python
from typing import Final, NamedTuple

import limits
from db import fetch_all
from limits import LIMIT

DEFAULT = "x"
FINAL: Final = "x"

class Row(NamedTuple):
    key: str

def run(key) -> None:
    fetch_all("SELECT %(key)s::TEXT AS key FROM users", Row, key=DEFAULT)
    fetch_all("SELECT %(key)s::TEXT AS key FROM users", Row, key=FINAL)
    fetch_all("SELECT %(key)s::TEXT AS key FROM users", Row, key=LIMIT)
    fetch_all("SELECT %(key)s::TEXT AS key FROM users", Row, key=limits.LIMIT)
    fetch_all("SELECT %(key)s::TEXT AS key FROM users", Row, key=limits.ALIAS)
    # error: [nullable-mismatch] "Row.key is str but query column 'key' can be NULL"
    fetch_all("SELECT %(key)s::TEXT AS key FROM users", Row, key=input())
    # error: [nullable-mismatch] "Row.key is str but query column 'key' can be NULL"
    fetch_all("SELECT %(key)s::TEXT AS key FROM users", Row, key=key)
```

### A column an INSERT writes from a parameter

A column an `INSERT` writes from a placeholder the call binds to a value that excludes
`None` is not NULL in the rows `RETURNING` gives back, as with a literal. A `BEFORE
INSERT` row trigger may change the row, so it keeps the column nullable.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[[tool.sqlty.functions]]
import-path = "db.fetch_all"
query-param = "query"
return-type-param = "row"
```

`schema.sql`:

```sql
CREATE TABLE items (id integer PRIMARY KEY, owner_id integer, label text);
CREATE TABLE logged (id integer PRIMARY KEY, owner_id integer);
CREATE FUNCTION stamp() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$;
CREATE TRIGGER stamp BEFORE INSERT ON logged FOR EACH ROW EXECUTE FUNCTION stamp();
```

`db.py`:

```python
from typing import Any

def fetch_all[T](query: str, row: type[T], /, **params: Any) -> list[T]: ...
```

```python
from typing import NamedTuple

from db import fetch_all

class Row(NamedTuple):
    id: int
    owner_id: int

def insert(owner_id: int, maybe: int | None) -> None:
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=owner_id)
    fetch_all("INSERT INTO items (id, owner_id) SELECT 1, %(owner_id)s RETURNING id, owner_id", Row, owner_id=owner_id)
    fetch_all(
        "WITH i AS (INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id)"
        " SELECT id, owner_id FROM i",
        Row,
        owner_id=owner_id,
    )
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=maybe)
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO logged (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=owner_id)
```

#### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
from typing import NamedTuple

from db import fetch_all

class Row(NamedTuple):
    id: int
    owner_id: int

def insert(owner_id: int, maybe: int | None) -> None:
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=owner_id)
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=maybe)
```

### A `NewType` argument

An argument annotated with a `NewType` has the type of its supertype, so it excludes
`None` as that does, whether the `NewType` is defined in the module, through another
`NewType`, or imported from a checked module. A function of the module named `NewType`
is not `typing.NewType`.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[[tool.sqlty.functions]]
import-path = "db.fetch_all"
query-param = "query"
return-type-param = "row"
```

`schema.sql`:

```sql
CREATE TABLE items (id integer PRIMARY KEY, owner_id integer, ref uuid);
```

`db.py`:

```python
from typing import Any

def fetch_all[T](query: str, row: type[T], /, **params: Any) -> list[T]: ...
```

`ids.py`:

```python
from typing import NewType
from uuid import UUID

OwnerId = NewType("OwnerId", int)
ChainId = NewType("ChainId", OwnerId)
Ref = NewType("Ref", UUID)
```

```python
import typing as t
from typing import NamedTuple, NewType
from uuid import UUID

import ids
from db import fetch_all
from ids import ChainId, OwnerId, Ref

LocalId = NewType("LocalId", int)
ChildId = NewType("ChildId", LocalId)
Spelled = t.NewType("Spelled", int)

def NotNewType(name: str, tp: object) -> object: ...

Fake = NotNewType("Fake", int)

class Row(NamedTuple):
    id: int
    owner_id: int

class RefRow(NamedTuple):
    ref: UUID

def insert(local: LocalId, child: ChildId, spelled: Spelled, imported: OwnerId, chained: ChainId, dotted: ids.OwnerId, fake: Fake) -> None:
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=local)
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=child)
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=spelled)
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=imported)
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=chained)
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=dotted)
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=LocalId(1))
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=fake)

def coalesce(local: LocalId, imported: OwnerId, ref: Ref) -> None:
    fetch_all("SELECT id, coalesce(owner_id, %(owner_id)s) AS owner_id FROM items", Row, owner_id=local)
    fetch_all("SELECT id, coalesce(owner_id, %(owner_id)s) AS owner_id FROM items", Row, owner_id=imported)
    fetch_all("SELECT coalesce(ref, %(ref)s) AS ref FROM items", RefRow, ref=ref)
    # error: [param-type-mismatch] "parameter 'ref' expects uuid.UUID, got int"
    fetch_all("SELECT coalesce(ref, %(ref)s) AS ref FROM items", RefRow, ref=imported)
```

#### What is not a `NewType`

A name that is not bound once to a `NewType` call, or one made of itself, keeps the
argument of unknown type, as does a `NewType` of a type that is not a class.

`kinds.py`:

```python
from typing import NewType

Listed = NewType("Listed", list[int])
```

```python
from typing import NamedTuple, NewType

import kinds
from db import fetch_all

Plain = 5
Twice = NewType("Twice", int)
Twice = NewType("Twice", int)
Keyword = NewType("Keyword", tp=int)
Loop = NewType("Loop", Loop)

class Row(NamedTuple):
    id: int
    owner_id: int

def insert(plain: Plain, twice: Twice, keyword: Keyword, loop: Loop, listed: kinds.Listed) -> None:
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=plain)
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=twice)
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=keyword)
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=loop)
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    fetch_all("INSERT INTO items (id, owner_id) VALUES (1, %(owner_id)s) RETURNING id, owner_id", Row, owner_id=listed)
```

#### Driver calls

The arguments of a driver call are read the same way.

```python
from typing import NamedTuple

import psycopg

from ids import OwnerId
from sqlty import SQL

class Row(NamedTuple):
    owner_id: int

def run(conn: psycopg.Connection, owner_id: OwnerId, maybe: OwnerId | None) -> None:
    conn.execute(SQL[Row]("--sql\nSELECT coalesce(owner_id, %(owner_id)s) AS owner_id FROM items"), {"owner_id": owner_id})
    # error: [nullable-mismatch] "Row.owner_id is int but query column 'owner_id' can be NULL"
    conn.execute(SQL[Row]("--sql\nSELECT coalesce(owner_id, %(owner_id)s) AS owner_id FROM items"), {"owner_id": maybe})
```
