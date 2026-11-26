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

## Binary columns are bytes

`BYTEA` and `BLOB` columns are `bytes`, checked like any other type.

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import NamedTuple, TypedDict
from sqlty import SQL

class File(TypedDict):
    data: int

class Blob(NamedTuple):
    data: str

class Raw(TypedDict):
    data: bytes

# error: [column-type-mismatch] "File.data is int but query column 'data' infers bytes"
a = SQL[File]("--sql\nSELECT 'x'::BYTEA AS data")
# error: [column-type-mismatch] "Blob.data is str but query column 'data' infers bytes"
b = SQL[Blob]("--sql\nSELECT 'x'::BYTEA AS data")
c = SQL[Raw]("--sql\nSELECT 'x'::BYTEA AS data")
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

## Forward references

A row type spelled as a string, as a forward reference to a class defined
later, is checked as the type it names.

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
users = SQL["User"]("--sql\nSELECT 1::INTEGER AS id")
# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
counts = SQL["tuple[str]"]("--sql\nSELECT 1::INTEGER")

class User(TypedDict):
    id: str
```

## A single value is never a row

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from sqlty import SQL

# error: [narrow-type] "SQL[int] should be SQL[tuple[int]]: a row is never a single value"
count = SQL[int]("--sql\nSELECT count(*)::INTEGER FROM users")
unknown = SQL[int]("--sql\nSELECT $1")
```

## Optional row types

The `None` is reported and the rest is checked as the row type.

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
import typing
from typing import NamedTuple, Optional, TypedDict, Union
from sqlty import SQL, SQLTemplate

class User(TypedDict):
    id: str

class Pair(NamedTuple):
    id: int
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
# error: [optional-row] "SQL[User | None] should be SQL[User]: a row is never None"
a = SQL[User | None]("--sql\nSELECT id::INTEGER FROM users")
# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
# error: [optional-row] "SQL[None | tuple[str]] should be SQL[tuple[str]]: a row is never None"
b = SQL[None | tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
# error: [optional-row] "SQL[typing.Optional[tuple[int]]] should be SQL[tuple[int]]: a row is never None"
c = SQL[typing.Optional[tuple[int]]]("--sql\nSELECT id::INTEGER FROM users")
# error: [optional-row] "SQL[Union[Pair, None]] should be SQL[Pair]: a row is never None"
d = SQL[Union[Pair, None]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [optional-row] "SQL[Pair | None] should be SQL[Pair]: a row is never None"
e = SQL["Pair | None"]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [optional-row] "SQLTemplate[tuple[int] | None] should be SQLTemplate[tuple[int]]: a row is never None"
f = SQLTemplate[tuple[int] | None](t"--sql\nSELECT id::INTEGER FROM users")
# error: [narrow-type] "SQL[int] should be SQL[tuple[int]]: a row is never a single value"
# error: [optional-row] "SQL[int | None] should be SQL[int]: a row is never None"
g = SQL[int | None]("--sql\nSELECT id::INTEGER FROM users")
```

## Optional row types - not a single `None` member

A `None` between other members, or one of a `Union` of several, is no `optional-row`: the
row type is a union, `unchecked-row-type`.

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import NamedTuple, Optional, Union
from sqlty import SQL

class Pair(NamedTuple):
    id: int
    name: str

# error: [unchecked-row-type] "row type Pair | None | tuple[int] is a union, which sqlty cannot check against the query"
a = SQL[Pair | None | tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [unchecked-row-type] "row type Union[Pair, tuple[int], None] is a union, which sqlty cannot check against the query"
b = SQL[Union[Pair, tuple[int], None]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
c = SQL[None]("--sql\nSELECT id::INTEGER FROM users")
# error: [unchecked-row-type] "row type None | None is a union, which sqlty cannot check against the query"
d = SQL[None | None]("--sql\nSELECT id::INTEGER FROM users")
# error: [unchecked-row-type] "row type Optional[Pair] | tuple[int] is a union, which sqlty cannot check against the query"
e = SQL[Optional[Pair] | tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [unchecked-row-type] "row type Union[None, Pair] | tuple[int] is a union, which sqlty cannot check against the query"
f = SQL[Union[None, Pair] | tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Optional row types - fix

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import Optional
from sqlty import SQL

a = SQL["tuple[int] | None"]("--sql\nSELECT id::INTEGER FROM users")
b = SQL[Optional[tuple[int]]]("--sql\nSELECT id::INTEGER FROM users")
c = SQL[None | tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: from typing import Optional
# fixed: from sqlty import SQL
# fixed:
# fixed: a = SQL["tuple[int]"]("--sql\nSELECT id::INTEGER FROM users")
# fixed: b = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# fixed: c = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Unchecked row types

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
import typing
from typing import Annotated, NamedTuple, TypeAlias, TypedDict, Union
from sqlty import SQL, SQLTemplate

class A(TypedDict):
    id: int

class B(NamedTuple):
    id: str

type Row = A | B
Old: TypeAlias = Union[A, B]
type Tagged = Annotated[A, "x"]
type One = A
type MaybeA = A | None

# error: [unchecked-row-type] "row type A | B is a union, which sqlty cannot check against the query"
a = SQL[A | B]("--sql\nSELECT id::INTEGER FROM t")
# error: [unchecked-row-type] "row type typing.Union[A, B] is a union, which sqlty cannot check against the query"
b = SQL[typing.Union[A, B]]("--sql\nSELECT id::INTEGER FROM t")
# error: [unchecked-row-type] "row type Row is a union, which sqlty cannot check against the query"
c = SQL[Row]("--sql\nSELECT id::INTEGER FROM t")
# error: [unchecked-row-type] "row type Old is a union, which sqlty cannot check against the query"
d = SQL[Old]("--sql\nSELECT id::INTEGER FROM t")
# error: [unchecked-row-type] "row type Annotated[B, "x"] is Annotated, which sqlty cannot check against the query"
e = SQL[Annotated[B, "x"]]("--sql\nSELECT id::INTEGER FROM t")
# error: [unchecked-row-type] "row type Tagged is Annotated, which sqlty cannot check against the query"
f = SQL[Tagged]("--sql\nSELECT id::INTEGER FROM t")
# error: [unchecked-row-type] "row type tuple[int] | tuple[str] is a union, which sqlty cannot check against the query"
g = SQL[tuple[int] | tuple[str]]("--sql\nSELECT id::INTEGER FROM t")
# error: [optional-row] "SQL[A | B | None] should be SQL[A | B]: a row is never None"
# error: [unchecked-row-type] "row type A | B is a union, which sqlty cannot check against the query"
h = SQL[A | B | None]("--sql\nSELECT id::INTEGER FROM t")
# error: [unchecked-row-type] "row type A | B is a union, which sqlty cannot check against the query"
i = SQLTemplate[A | B](t"--sql\nSELECT id::INTEGER FROM t")
j = SQL[One]("--sql\nSELECT id::INTEGER FROM t")
k = SQL[typing.Any]("--sql\nSELECT id::INTEGER FROM t")
l = SQL[Annotated[typing.Any, "dynamic"]]("--sql\nSELECT id::INTEGER FROM t")
# error: [narrow-type] "SQL[int | str] should be SQL[tuple[int]]: a row is never a single value"
m = SQL[int | str]("--sql\nSELECT id::INTEGER FROM t")
n = SQL[MaybeA]("--sql\nSELECT id::INTEGER FROM t")
```

## Unchecked row types - rule disabled

```toml
[tool.sqlty]
dialect = "postgres"

[tool.sqlty.rules]
unchecked-row-type = "ignore"
```

```python
from typing import TypedDict
from sqlty import SQL

class A(TypedDict):
    id: int

class B(TypedDict):
    id: str

type Row = A | B

# error:
a = SQL[A | B]("--sql\nSELECT id::INTEGER FROM t")
b = SQL[Row]("--sql\nSELECT id::INTEGER FROM t")
```
