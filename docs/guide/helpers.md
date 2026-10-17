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

The marker and `Annotated` are recognized through any import form, whatever
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

## Methods

Methods are marked the same way. `self` and `cls` are not counted when the
arguments are bound. A call is checked when the receiver is evident: the class
(`Db.fetch_one(...)`, for static and class methods), `self` inside the class,
a variable assigned `Db(...)` or annotated `db: Db` in the same scope (or the
module), and `self.db` when the class body annotates it or `__init__` assigns
`Db(...)`. Other receivers, and inherited methods, are skipped.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def fetch_all[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

    @staticmethod
    def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

db = Db()
# error: [column-type-mismatch]
ids = db.fetch_all("SELECT name FROM users", int)
# error: [column-type-mismatch]
one = Db.fetch_one("SELECT name FROM users", int)
```

## Helpers you cannot annotate

A helper whose signature you cannot edit (a library, generated code) is
registered in `pyproject.toml` instead:

```toml
[[tool.sqlty.functions]]
import-path = "pkg.db.fetch_all"   # or "pkg.db.Db.fetch_all" for a method
query-param = "query"
return-type-param = "form"
```

`query-param` names the parameter that holds the query and `return-type-param`
the one holding the row type. When `pkg.db` is among the checked paths, both are
looked up in the definition, so calls may pass them by position; otherwise
they are checked only when passed by keyword (`fetch_all(query="...", form=int)`).
For an `@overload`ed helper, positions come from the implementation, or from the
first overload naming both parameters when the implementation does not.

## Helpers in other modules

`sqlty check` indexes the helpers of every file it is given before checking
calls, so a helper imported from another module of the checked paths is
resolved by its module name: `from pkg.db import fetch_all [as fa]`,
`import pkg.db as db` with `db.fetch_all(...)`, and relative imports.
Only files that mention `Annotated` and `SQL` are parsed for the index.

Pass the directory that holds the helper together with the code that calls it
(`sqlty check .`, not just the calling file). A helper re-exported by a package
`__init__` (`from .db import fetch_all`) is found through the package too:
`from pkg import fetch_all` reaches it.

From Python, `sqlty.core.check_source(source, helpers=index, module="pkg.app")`
takes a `sqlty.core.HelperIndex` filled with `index.add("pkg.db", db_source)`.

`sqlty rewrite` and `sqlty strip` never touch these calls.

## psycopg `sql.SQL` queries

With `psycopg` installed (`sqlty[psycopg]`), a `psycopg.sql` composition passed
to the query parameter is checked too, so helpers annotated
`Annotated[LiteralString | sql.Composable, SQL]` cover queries with dynamic
identifiers. `sql.SQL(...)`, `.format(...)`, `.join(...)`, `sql.Composed([...])`,
`sql.Identifier`, `sql.Placeholder` and `sql.Literal` are recognized through
any import form (`from psycopg import sql`, `from psycopg.sql import SQL as
PgSQL`, `import psycopg.sql`).

When every part is a literal, sqlty rebuilds the object from the source with
the real `psycopg.sql` classes (your code is never run) and renders it with
`as_string(None)`, which needs no connection since psycopg 3.2. A name bound
once in the same function or module is followed, so shared fragments and
module-level constants work. `sql.Literal(x)` renders as `NULL`, like a
placeholder, whatever `x` is.

```python
from typing import Annotated, LiteralString
from psycopg import sql
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString | sql.Composable, SQL], form: type[T], /) -> list[T]: ...

USERS = sql.Identifier("users")
query = sql.SQL("SELECT {cols} FROM {table} WHERE id = {id}").format(
    cols=sql.SQL(", ").join([sql.Identifier("id"), sql.Identifier("name")]),
    table=USERS,
    id=sql.Placeholder("id"),
)

# error: [column-type-mismatch]
rows = fetch_all(query, tuple[int, int])
```

A name imported from another checked module (`from .columns import
USER_COLUMNS`, or `columns.USER_COLUMNS` after `import pkg.columns as columns`)
resolves the same way when that module binds it once at module level to a
literal string or a composition built only from literals. Each module is parsed
once, however many files import from it.

A part that is not a literal (`sql.Identifier(column)` with a variable) makes
the query `unchecked-query`. Without psycopg installed these calls are skipped.

A `--sql` marker inside `sql.SQL("--sql\n...")` is fine; `rewrite` never wraps
a string inside `sql.SQL(...)` and `stale-rewrite` does not report it.
