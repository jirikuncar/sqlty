# Helper calls in nested scopes

Bodies that never mention a helper are not searched for calls; the others are,
at any depth.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(255));
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## Nested function in a method

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

class Repo:
    def names(self):
        def inner():
            # error: [column-type-mismatch]
            return fetch_all("SELECT name FROM users", int)
        return inner()

def unrelated(x):
    return [a for a in range(x)]
```

## Calls that are not followed

A helper called through a subscript, a `**` mapping for a helper's `**params`, a query
built by a call, one looked up by a key that is not a string and a generator argument
leave their queries unchecked.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...
def run(query: Annotated[LiteralString, SQL], **params: object) -> None: ...
def build() -> LiteralString: ...

helpers = [fetch_all]
a = helpers[0]("SELECT name FROM users", int)
run("SELECT name FROM users WHERE id = %(id)s", **{"id": 1})
b = fetch_all(build(), int)
c = fetch_all(q for q in ["SELECT name FROM users"])
QUERIES = {1: "SELECT name FROM users"}
d = fetch_all(QUERIES[1], int)

def outer(query: Annotated[LiteralString, SQL]) -> None:
    class Inner:
        def method(self) -> None:
            fetch_all(query, int)
# error:
```
