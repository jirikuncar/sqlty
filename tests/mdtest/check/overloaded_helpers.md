# Overloaded helpers

The implementation gives the parameter positions, or the first overload naming
both the query and the row parameter when the implementation does not.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(255));
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## Configured

```toml
[[tool.sqlty.functions]]
import_path = "pkg.db.fetch_all"
query_param = "query"
return_type_param = "form"

[[tool.sqlty.functions]]
import_path = "pkg.db.fetch_conn"
query_param = "query"
return_type_param = "form"

[[tool.sqlty.functions]]
import_path = "pkg.db.Db.fetch_all"
query_param = "query"
return_type_param = "form"
```

`pkg/db.py`:

```python
from typing import Any, LiteralString, overload

@overload
def fetch_all(query: LiteralString, /, **params: Any) -> list[tuple[Any, ...]]: ...
@overload
def fetch_all[T](query: LiteralString, form: type[T], /, **params: Any) -> list[T]: ...
def fetch_all(query, form=None, /, **params):
    return []

@overload
async def fetch_conn(conn: Any, query: LiteralString, /) -> list[Any]: ...
@overload
async def fetch_conn[T](conn: Any, query: LiteralString, form: type[T], /) -> list[T]: ...
async def fetch_conn(conn, query, form=None, /):
    return []

class Db:
    @overload
    def fetch_all(self, query: LiteralString, /) -> list[Any]: ...
    @overload
    def fetch_all[T](self, query: LiteralString, form: type[T], /) -> list[T]: ...
```

### First overload without the row parameter

```python
from pkg.db import fetch_all

# error: [column-type-mismatch]
ids = fetch_all("SELECT name FROM users", int)
```

### Connection parameter and `await`

```python
from pkg.db import fetch_conn

async def names(conn):
    # error: [column-type-mismatch]
    return await fetch_conn(conn, "SELECT name FROM users", int)
```

### Method with only overloads

```python
from pkg.db import Db

db = Db()
# error: [column-type-mismatch]
ids = db.fetch_all("SELECT name FROM users", int)
```

## Annotated, row overload first

`pkg/db.py`:

```python
from typing import Annotated, Any, LiteralString, overload
from sqlty import SQL

@overload
def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T: ...
@overload
def fetch_one(query: Annotated[LiteralString, SQL], /) -> Any: ...
```

```python
from pkg.db import fetch_one

# error: [column-type-mismatch]
one = fetch_one("SELECT name FROM users", int)
```
