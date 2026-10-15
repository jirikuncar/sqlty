# Check literals passed to annotated query parameters

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    total NUMERIC(10, 2) NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT NOW()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Helper with an annotated query parameter

```python
from typing import Annotated, LiteralString, TypedDict
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /, **params: object) -> list[T]: ...

class User(TypedDict):
    id: int
    name: str

users = fetch_all("SELECT id, name FROM users", User)

# error: [column-type-mismatch] "User.name is str but query column 'name' infers int"
broken = fetch_all("SELECT id, id AS name FROM users", User)
```

## Keyword argument and implicit concatenation

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_value[T](query: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...

# error: [column-type-mismatch]
n = fetch_value(form=str, query="SELECT count(*) " "FROM users")
```

## Import aliases

```python
import typing as t
import sqlty as sy

def fetch_all[T](query: t.Annotated[t.LiteralString, sy.SQL], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
ids = fetch_all("SELECT name FROM users", int)
```

```python
from typing import Annotated as A, LiteralString
from sqlty import SQL as Q

def fetch_all[T](query: A[LiteralString, Q], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
ids = fetch_all("SELECT name FROM users", int)
```

The marker need not match the configured `import-style`:

```python
from typing_extensions import Annotated
import sqlty

def fetch_all[T](query: Annotated[str, sqlty.SQL], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
ids = fetch_all("SELECT name FROM users", int)
```

## Scalar and tuple rows

A scalar takes one column, a tuple one per item.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

names = fetch_all("SELECT name FROM users", str)
pairs = fetch_all("SELECT id, name FROM users", tuple[int, str])
maybe = fetch_all("SELECT email FROM users", str | None)

# error: [column-count-mismatch] "int takes 1 column but query returns 2 columns"
ids = fetch_all("SELECT id, name FROM users", int)

# error: [column-count-mismatch] "tuple[int, str] has 2 items but query returns 3 columns"
rows = fetch_all("SELECT id, name, email FROM users", tuple[int, str])

# error: [column-type-mismatch] "tuple[int, int] item 1 is int but query column 'name' infers str"
bad = fetch_all("SELECT id, name FROM users", tuple[int, int])
```

## Row types defined in the file

```python
from dataclasses import dataclass
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T: ...

@dataclass
class Order:
    id: int
    total: str

# error: [column-type-mismatch] "Order.total is str but query column 'total' infers"
order = fetch_one("SELECT id, total FROM orders", Order)

# error: [column-count-mismatch] "Order has 2 fields but query returns 1 columns"
short = fetch_one("SELECT id FROM orders", Order)
```

## Without a row type parameter

Only `select-star` applies.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def run(query: Annotated[LiteralString, SQL], /) -> None: ...

run("SELECT id, name FROM users")

# error: [select-star]
run("SELECT * FROM users")
```

## Qualified and RETURNING stars in helper calls

`select-star` finds the same stars in a helper's query as in `SQL[T]`.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def run(query: Annotated[LiteralString, SQL], /) -> None: ...

# error: [select-star] "SELECT u.* used — expand to explicit column list"
run("SELECT u.*, o.id FROM users u JOIN orders o ON o.user_id = u.id")
# error: [select-star] "RETURNING * used — expand to explicit column list"
run("DELETE FROM orders WHERE id = 1 RETURNING *")
# error: [select-star] "WITH u AS (SELECT *) used — expand to explicit column list"
run("WITH u AS (SELECT * FROM users) SELECT id FROM u")
run("SELECT id FROM users WHERE EXISTS (SELECT * FROM orders)")
```

## Calls that are not checked

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...
def plain[T](query: LiteralString, form: type[T], /) -> list[T]: ...

text = "SELECT name FROM users"
a = fetch_all(text, int)
b = plain("SELECT name FROM users", int)
c = fetch_all("SELECT name FROM users", Unknown)
```

## rewrite and strip leave the calls alone

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

ids = fetch_all("SELECT name FROM users", str)
# rewritten: from typing import Annotated, LiteralString
# rewritten:
# rewritten: def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...
# rewritten:
# rewritten: ids = fetch_all("SELECT name FROM users", str)
# stripped: from typing import Annotated, LiteralString
# stripped: from sqlty import SQL
# stripped:
# stripped: def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...
# stripped:
# stripped: ids = fetch_all("SELECT name FROM users", str)
```
