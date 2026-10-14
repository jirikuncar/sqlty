# Python API

## `sqlty`

```python
from sqlty import SQL, UNTYPED, row_form, sqlty
```

### `SQL[T]`

```python
class SQL[T](str): ...
```

A `str` subclass parameterized by the row type `T`. It adds no methods and
works anywhere a string works. `sqlty rewrite` creates instances:

```python
query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

`T` is `tuple[...]` as inferred, a [row type](../guide/row-types.md) you
write, or [`UNTYPED`](#untyped). String operations return plain `str`.

### `UNTYPED`

A [PEP 661](https://peps.python.org/pep-0661/) sentinel used as the row type
of a query sqlty could not type: `SQL[UNTYPED]`. Type checkers reject using
its rows as a concrete type. See [Untyped queries](../guide/untyped.md).

### `row_form(query[, default])`

```python
def row_form[T](query: SQL[T], /) -> TypeForm[T]: ...
def row_form[T, D](query: SQL[T], default: D, /) -> TypeForm[T] | D: ...
```

Returns the `T` an instance was built with, read from `__orig_class__`:

```pycon
>>> from sqlty import SQL, row_form
>>> row_form(SQL[tuple[int, str]]("--sql\nSELECT 1::INTEGER, 'a'::TEXT"))
tuple[int, str]
>>> print(row_form(SQL("--sql\nSELECT 1::INTEGER"), None))
None
```

Plain strings, `SQL("...")`, annotation-only `q: SQL[T] = "..."` and
stripped sources carry no runtime type: `row_form` raises `TypeError` for
them unless `default` is given.

### `@sqlty`

```python
@sqlty
def sql(query: str) -> SQL[Any]: ...

@sqlty(schema="schema.sql")
def sql(query: str) -> SQL[Any]: ...
```

Marks a helper function for [stub generation](../guide/stubs.md). `schema`
is a path relative to the module and is used to infer types for the stub,
unless the helper's `[[tool.sqlty.functions]]` entry or a
`[tool.sqlty.schemas]` `"module.function"` key sets one (see
[Schemas](../guide/stubs.md#schemas)).
Decorating a function twice raises `TypeError`.

Without `SQLTY_AUTO_GENERATE`, the decorator returns the function unchanged.
With it, a function registered in `[[tool.sqlty.functions]]` is wrapped to
record each query it receives (its `query_param` argument, defaults applied, and the
`return_type_param` argument if configured) and passes all arguments through;
an unregistered one triggers a warning.

`async def` helpers are supported and get `async def` overloads. The query is
recorded when the helper is called, before the coroutine is awaited.

## `sqlty.drivers.psycopg`

Requires `sqlty[psycopg]`. See [psycopg](../guide/psycopg.md).

```python
from sqlty.drivers.psycopg import execute, row_factory, scalar
```

### `execute(conn, query, params=None)`

```python
def execute[T](conn: Connection[Any], query: SQL[T], params: Params | None = None) -> Cursor[T]: ...
```

Runs `query` on a new cursor whose row factory is
`row_factory(row_form(query))`, or `tuple_row` when the query has no runtime
row type.

### `row_factory(form)`

```python
def row_factory[T](form: TypeForm[T]) -> BaseRowFactory[T]: ...
```

| `form` | Returns |
|--------|---------|
| `tuple[...]` | `tuple_row` |
| `TypedDict`, `dict[...]` | `dict_row` |
| any other class (dataclass, `NamedTuple`, Pydantic model) | `class_row(form)` |

Raises `TypeError` for anything else.

### `scalar(conn, query, [form,] params=None)`

```python
def scalar[T](conn: Connection[Any], query: SQL[tuple[T]], /, params: Params | None = None) -> T: ...
def scalar[T](conn: Connection[Any], query: LiteralString | SQL[Any], form: TypeForm[T], /, params: Params | None = None) -> T: ...
```

Returns the only column of the first row, typed from `SQL[tuple[T]]` or from
`form`. `form` accepts any type expression, including unions and `Literal`,
and is not checked at runtime. Raises `LookupError` when the query returns no
rows and `ValueError` when the row has more than one column.

## `sqlty.core`

```python
from sqlty.core import (
    analyze_select,
    check_source,
    expand_star,
    fix_source,
    infer_type,
    returns_rows,
    rewrite_source,
    strip_source,
)
```

The functions behind the CLI, implemented by the Rust [engine](engine.md).
Use them to build your own tooling:

```pycon
>>> from sqlty.core import infer_type, rewrite_source, strip_source
>>> infer_type("SELECT id::INTEGER, name::TEXT FROM users")
'tuple[int, str]'
>>> infer_type("SELECT id FROM users", schema_sql="CREATE TABLE users (id INTEGER);")
'tuple[int]'
>>> source = rewrite_source('q = "--sql\\nSELECT 1::INTEGER"')
>>> print(source)
from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
>>> print(strip_source(source))
q = "--sql\nSELECT 1::INTEGER"
```

| Function | Returns |
|----------|---------|
| `infer_type(sql, *, dialect="postgres", schema_sql=None, preview=False)` | Row type as a string, or `None` |
| `analyze_select(sql, *, dialect="postgres", schema_sql=None, preview=False)` | Result columns as `ColumnInfo(name, python_type, sql_type)`, or `None` |
| `returns_rows(sql, *, dialect="postgres")` | Whether the statement yields rows (unparseable SQL counts) |
| `expand_star(sql, *, dialect="postgres", schema_sql=None)` | Query with `*` replaced by cast columns, or `None` |
| `rewrite_source(source, *, dialect="postgres", schema_sql=None, import_style=None, untyped=False, preview=False)` | Rewritten source |
| `strip_source(source, *, import_style=None)` | Stripped source |
| `fix_source(source, *, dialect="postgres", schema_sql=None, import_style=None, untyped=False, rules=None, preview=False)` | Source with `sqlty check --fix` applied |
| `check_source(source, *, dialect="postgres", schema_sql=None, rules=None, import_style=None, preview=False)` | List of diagnostics: dicts with `rule`, `message`, `severity`, `line` |
