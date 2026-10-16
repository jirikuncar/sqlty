# Type Annotation Checks

## Column count mismatch — too few fields

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int

# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Column count mismatch — too many fields

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    email: str

# error: [column-count-mismatch] "User has 3 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Column type mismatch

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Column name mismatch

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    user_id: int
    username: str

# error: [column-name-mismatch] "User.user_id doesn't match query column 'id'"
# error: [column-name-mismatch] "User.username doesn't match query column 'name'"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## All fields match — no error

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Count mismatch rule disabled — no error

```toml
[tool.sqlty]
dialect = "postgres"
[tool.sqlty.rules]
column-count-mismatch = "ignore"
```

```python
from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    id: int

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Type mismatch rule disabled — no error

```toml
[tool.sqlty]
dialect = "postgres"
[tool.sqlty.rules]
column-type-mismatch = "ignore"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Name mismatch when count also mismatches — count reported first

Count mismatch takes priority over name/type checks.

```toml
[tool.sqlty]
dialect = "postgres"
[tool.sqlty.rules]
column-count-mismatch = "error"
column-type-mismatch = "error"
```

```python
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    wrong_name: str

# error: [column-count-mismatch] "Row has 1 fields but query returns 2 columns"
q = SQL[Row]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Functional TypedDict and NamedTuple

The functional forms are checked like the class forms.

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
import typing
from typing import NamedTuple, TypedDict
from sqlty import SQL

R = TypedDict("R", {"a": str})
N = NamedTuple("N", [("a", str)])
P = typing.NamedTuple("P", (("a", int), ("b", str)))
Partial = TypedDict("Partial", {"a": int, "b": str}, total=False)

class Child(R):
    b: int

# error: [column-type-mismatch] "R.a is str but query column 'a' infers int"
q1 = SQL[R]("--sql\nSELECT a::INTEGER AS a")
# error: [column-type-mismatch] "N.a is str but query column 'a' infers int"
q2 = SQL[N]("--sql\nSELECT a::INTEGER AS a")
# error: [column-count-mismatch] "P has 2 fields but query returns 1 columns"
q3 = SQL[P]("--sql\nSELECT a::INTEGER AS a")
q4 = SQL[P]("--sql\nSELECT 1::INTEGER AS a, 'x'::TEXT AS b")
q5 = SQL[Partial]("--sql\nSELECT 1::INTEGER AS a")
# error: [column-type-mismatch] "Child.b is int but query column 'b' infers str"
q6 = SQL[Child]("--sql\nSELECT 'x'::TEXT AS a, 'y'::TEXT AS b")
```

## Protocol row types

A `Protocol` says which attributes a row has, not how it is built: rows from
a row factory such as psycopg's `namedtuple_row` satisfy it. Columns are
matched to attributes by name, and the query may return more columns than the
protocol lists.

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import Protocol
from sqlty import SQL

class Named(Protocol):
    id: int
    name: str

a = SQL[Named]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS name, 2::INTEGER AS extra")
# error: [column-type-mismatch] "Named.id is int but query column 'id' infers str"
b = SQL[Named]("--sql\nSELECT 'x'::TEXT AS id, 'x'::TEXT AS name")
# error: [column-count-mismatch] "Named has 2 required attributes but query returns 1 columns"
c = SQL[Named]("--sql\nSELECT 1::INTEGER AS id")
# error: [typeddict-key-mismatch] "Named.name is required but missing from query"
d = SQL[Named]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS title")
```
