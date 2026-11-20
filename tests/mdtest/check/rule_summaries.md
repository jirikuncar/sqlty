# Rule summaries

One short example per rule, with two rules raised to `error`.

```toml
[tool.sqlty.rules]
column-name-mismatch = "error"
stale-type = "error"
```

## Row type rules

### column-count-mismatch

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int

# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### column-type-mismatch

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### column-name-mismatch

```python
from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    user_id: int
    name: str

# error: [column-name-mismatch] "User.user_id doesn't match query column 'id'"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### nullable-mismatch

```toml
[tool.sqlty]
schema = "schema.sql"
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

### json-shape-mismatch

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, total NUMERIC(10, 2) NOT NULL);
```

```python
from typing import TypedDict
from sqlty import SQL

class Doc(TypedDict):
    id: int
    total: str

class Order(TypedDict):
    doc: Doc

# error: [json-shape-mismatch] "Order.doc is Doc but query column 'doc' builds JSON that does not fit: doc.total is str but the query builds a number"
q = SQL[Order]("--sql\nSELECT jsonb_build_object('id', id, 'total', total) AS doc FROM orders")
```

### redundant-optional

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-optional = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str | None

# error: [redundant-optional] "User.name is str | None but query column 'name' is never NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

### literal-mismatch

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'refunded'))
);
```

```python
from typing import Literal, TypedDict
from sqlty import SQL

class Order(TypedDict):
    id: int
    status: Literal["pending", "paid"]

# error: [literal-mismatch] "Order.status is Literal['pending', 'paid'] but query column 'status' can be 'refunded'"
q = SQL[Order]("--sql\nSELECT id, status FROM orders")
```

### redundant-literal

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-literal = "warn"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid'))
);
```

```python
from typing import Literal, TypedDict
from sqlty import SQL

class Order(TypedDict):
    id: int
    status: Literal["pending", "paid", "refunded"]

# error: [redundant-literal] "Order.status is Literal['pending', 'paid', 'refunded'] but query column 'status' is never 'refunded'"
q = SQL[Order]("--sql\nSELECT id, status FROM orders")
```

### typeddict-key-mismatch

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

### typeddict-inheritance

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

## Parameter rules

### param-type-mismatch

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

# error: [param-type-mismatch] "parameter 1 expects int, got bytes"
execute("SELECT 1 FROM users LIMIT %s", (b"1",))
```

### param-count-mismatch

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...]) -> None: ...

# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
execute("SELECT 1 FROM users WHERE id = %s AND name = %s", (1,))
```

### extra-param

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: dict[str, object]) -> None: ...

# error: [extra-param] "call passes extra parameter 'name'"
execute("SELECT 1 FROM users WHERE id = %(id)s", {"id": 1, "name": "a"})
```

### params-type-mismatch

```python
from sqlty import SQL

# error: [params-type-mismatch] "tuple[int, int] has 2 items but the query has 1 parameter"
q = SQL[tuple[int], tuple[int, int]]("--sql\nSELECT 1 FROM users LIMIT %s")
```

## Query rules

### select-star-no-schema

```python
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[int]]("--sql\nSELECT * FROM users")
```

### select-star

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL

# error: [select-star] "SELECT * used — expand to explicit column list"
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
```

### inline-sql-comment

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty check --fix"
q = "--sql SELECT id::INTEGER FROM users"
```

### mysql-sql-marker

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error: [mysql-sql-marker] "MySQL reads "--sql" as an expression, not a comment; use "-- sql""
q = SQL[tuple[int]]("--sql\nSELECT CAST(id AS SIGNED) FROM users")
```

### unparsed-query

```python
from sqlty import SQL

# error: [unparsed-query] "found: DOCUMENT"
q = SQL[tuple[str]]("--sql\nSELECT NULL::xml IS DOCUMENT")
```

### sqruff

```toml
[tool.sqlty.rules]
sqruff = "warn"
```

```python
from sqlty import SQL

# error: [sqruff] "AM06: Inconsistent column references in 'GROUP BY/ORDER BY' clauses."
q = SQL[tuple[str]]("--sql\nSELECT name FROM users GROUP BY 1 ORDER BY name")
```

### unknown-database

```toml
[tool.sqlty.databases.analytics]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error: [unknown-database] "database `analytic` is not configured; did you mean `analytics`?"
q = SQL[tuple[int]]("--sql sqlty:db=analytic\nSELECT 1::INTEGER")
```

## Rewrite rules

### stale-rewrite

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = "--sql\nSELECT id::INTEGER FROM users"
```

### stale-type

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int | None]] should be SQL[tuple[int]]"
q = SQL[tuple[int | None]]("--sql\nSELECT id::INTEGER FROM users")
```

### narrow-type

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [narrow-type] "SQL[int] should be SQL[tuple[int]]"
count = SQL[int]("--sql\nSELECT count(*)::INTEGER FROM users")
```

### fstring-skipped

```python
# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
q = f"--sql\nSELECT {column}::INTEGER FROM users"
```

### tstring-skipped

```python
user_id = 1
# error: [tstring-skipped] "t-string with --sql prefix skipped (the field {user_id!r} changes the SQL)"
q = t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id!r}"
```

### unchecked-query

```python
from sqlty import SQL

# error: [unchecked-query] "--sql query is not plain string literals, so it is not checked or rewritten"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM " + table)
```

### dynamic-query

```toml
[tool.sqlty]
preview = false

[tool.sqlty.rules]
dynamic-query = "warn"
```

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...


NAMES = "SELECT name FROM users"
# error: [dynamic-query] "query is not a string literal, so it is not checked"
names = fetch_all(NAMES, str)
```

### untyped-query

```python
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type: columns `x`, `y` are untyped; add casts (for example id::INTEGER) or a schema"
q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```
