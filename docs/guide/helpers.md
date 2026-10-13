# Query helpers

Code that sends queries through a helper function needs no `--sql` marker and
no `SQL[T]` wrapper at the call sites. Mark the helper's query parameter with
`Annotated[LiteralString, SQL]` and `sqlty check` checks every string literal
passed to it. The row type comes from the helper's `type[T]` parameter.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(255));
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## Marking the helper

The query parameter is annotated with the `sqlty.SQL` class as `Annotated`
metadata. The parameter annotated `type[T]` (or `TypeForm[T]`), where `T` also
appears in the return type, is the row type:

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

Calls pass the query and the row type by position or by keyword, and a query
may be several adjacent string literals. A query that is not a literal (a
variable, an f-string) is skipped.

## Row types

The row type is checked like the one in `SQL[T]` (see [Row types](row-types.md)),
including types defined in the same file. A scalar takes one column and
`tuple[A, B]` one column per item:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

names = fetch_all("SELECT name FROM users", str)

# error: [column-type-mismatch] "tuple[int, int] item 1 is int but query column 'name' infers str"
rows = fetch_all("SELECT id, name FROM users", tuple[int, int])
```

A helper without such a parameter is still parsed, and `select-star` applies.
Row types that are not known (a class from another module, `dict`) are not
checked.

## Import styles

The marker and `Annotated` are recognised through any import form, whatever
`import-style` says: `from sqlty import SQL as Q`, `import sqlty as sy`
(`sy.SQL`), `from typing import Annotated as A`, `import typing as t` and
`typing_extensions`.

```python
import typing as t
import sqlty as sy

def fetch_all[T](query: t.Annotated[t.LiteralString, sy.SQL], form: type[T], /) -> list[T]: ...

# error: [column-type-mismatch]
ids = fetch_all("SELECT name FROM users", int)
```

## Helpers in other modules

`sqlty check` indexes the helpers of every file it is given before checking
calls, so a helper imported from another module of the checked paths is
resolved by its module name: `from pkg.db import fetch_all [as fa]`,
`import pkg.db as db` with `db.fetch_all(...)`, and relative imports.
Only files that mention `Annotated` and `SQL` are parsed for the index.

Pass the directory that holds the helper together with the code that calls it
(`sqlty check .`, not just the calling file). Methods (`self`, `cls`) and
helpers re-exported through another module are not followed.

From Python, `sqlty.core.check_source(source, helpers=index, module="pkg.app")`
takes a `sqlty.core.HelperIndex` filled with `index.add("pkg.db", db_source)`.

`sqlty rewrite` and `sqlty strip` never touch these calls.
