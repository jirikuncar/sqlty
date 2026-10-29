# Python API

## `sqlty`

```python
from sqlty import SQL, UNTYPED, SQLTemplate, row_form, sqlty
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

### `SQLTemplate[T]`

```python
class SQLTemplate[T]:
    template: string.templatelib.Template
    def __init__(self, template: Template, /) -> None: ...
```

A [t-string](../guide/writing-queries.md#t-strings) query whose rows are `T`.
`Template` cannot be subclassed, so the template is kept in `template`: pass
that to a driver that runs templates (psycopg 3.3), or the whole query to
[`execute`](#sqltydriverspsycopg) and `scalar`. `sqlty rewrite` creates instances:

```pycon
>>> from sqlty import SQLTemplate, row_form
>>> user_id = 1
>>> query = SQLTemplate[tuple[int]](t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id}")
>>> query.template.values
(1,)
>>> row_form(query)
tuple[int]
```

### `UNTYPED`

A [PEP 661](https://peps.python.org/pep-0661/) sentinel used as the row type
of a query sqlty could not type: `SQL[UNTYPED]`. Type checkers reject using
its rows as a concrete type. See [Untyped queries](../guide/untyped.md).

### `row_form(query[, default])`

```python
def row_form[T](query: SQL[T] | SQLTemplate[T], /) -> TypeForm[T]: ...
def row_form[T, D](query: SQL[T] | SQLTemplate[T], default: D, /) -> TypeForm[T] | D: ...
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
    analyze_params,
    analyze_select,
    check_source,
    expand_star,
    explain_select,
    fix_source,
    infer_type,
    load_schema,
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
>>> infer_type("SELECT id FROM users", schema_sql="CREATE TABLE users (id INTEGER NOT NULL);")
'tuple[int]'
>>> source = rewrite_source('q = "--sql\\nSELECT 1::INTEGER"')
>>> print(source)
from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
>>> print(strip_source(source))
q = "--sql\nSELECT 1::INTEGER"
```

`analyze_params` types each placeholder (`%s`, `%(name)s`, `$1`, `?`, `:name`)
from its context. `nullable` is `False` for a parameter stored in a `NOT NULL`
column, so `None` is not a valid argument:

```pycon
>>> from sqlty.core import analyze_params
>>> schema = "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, age INTEGER)"
>>> for p in analyze_params("INSERT INTO users (id, name, age) VALUES (%s, %s, %s)", schema_sql=schema):
...     print(p.number, p.python_type, p.nullable)
1 int False
2 str False
3 int True
>>> analyze_params("SELECT name FROM users WHERE id = ANY(%(ids)s)", schema_sql=schema)
[ParamInfo(number=1, name='ids', python_type='list[int]', sql_type='ARRAY<INT>', nullable=None, cast=False, untyped_sql_type=None)]
```

`dialect` takes `postgres` (the default), `mysql`, `sqlite` or another
sqlparser dialect, in any case. An unknown name raises `ValueError` rather than
reading the query as generic SQL:

```pycon
>>> infer_type("SELECT 1::INTEGER", dialect="PostgreSQL")
'tuple[int]'
>>> infer_type("SELECT 1", dialect="postgress")  # doctest: +ELLIPSIS
Traceback (most recent call last):
ValueError: unknown dialect "postgress"; expected one of: postgres, mysql, sqlite, ...
```

`schema_sql` takes the schema's SQL text, or a `Schema` from `load_schema`.
Text is parsed once and cached, but each call still finds it by comparing the
whole text. When you check many queries against one schema, load it once and
pass the handle:

```pycon
>>> from sqlty.core import load_schema
>>> schema = load_schema("CREATE TABLE users (id INTEGER NOT NULL, name TEXT);")
>>> schema
Schema(dialect="postgres")
>>> infer_type("SELECT id, name FROM users", schema_sql=schema)
'tuple[int, str | None]'
```

`check_source` returns what `sqlty check` reports. Each diagnostic carries the
UTF-8 byte offsets of what it is about, `start` to `end`, and the edits of
`sqlty check --fix` as `(start, end, replacement)`. Editors use them to underline
the row type or the query and to offer the fix:

```pycon
>>> from sqlty.core import check_source
>>> source = 'q = SQL[tuple[int]]("--sql\\nSELECT 1.5::REAL")'
>>> [diagnostic] = check_source(source)
>>> diagnostic["rule"], diagnostic["message"]
('narrow-type', 'SQL[tuple[int]] should be SQL[tuple[float]]')
>>> source.encode()[diagnostic["start"] : diagnostic["end"]]
b'tuple[int]'
>>> diagnostic["fix"]
[(8, 18, 'tuple[float]')]
```

!!! note "Offsets count bytes, not characters"
    Slice `source.encode()`, not `source`: the two differ once a line holds a
    character outside ASCII.

| Function | Returns |
|----------|---------|
| `load_schema(schema_sql, *, dialect="postgres", types=None)` | A parsed `Schema`, for any `schema_sql` argument; `types` is [`[tool.sqlty.types]`](configuration.md#types) |
| `infer_type(sql, *, dialect="postgres", schema_sql=None)` | Row type as a string, or `None` |
| `analyze_select(sql, *, dialect="postgres", schema_sql=None)` | Result columns as `ColumnInfo(name, python_type, sql_type)`, or `None` |
| `explain_select(sql, *, dialect="postgres", schema_sql=None)` | Per column, `ColumnRules(name, type_rule, nullability_rule)`: the [inference rules](engine.md#inference-rules) (`InferenceRule(id, reference, reasoning)`) behind it, or `None` |
| `json_shapes(sql, *, dialect="postgres", schema_sql=None)` | `(column, shape)` pairs for columns that build JSON of a known shape |
| `analyze_params(sql, *, dialect="postgres", schema_sql=None)` | Query parameters as `ParamInfo(number, name, python_type, sql_type, nullable, cast, untyped_sql_type)`, or `None` |
| `returns_rows(sql, *, dialect="postgres")` | Whether the statement yields rows (unparseable SQL counts) |
| `expand_star(sql, *, dialect="postgres", schema_sql=None)` | Query with `*` replaced by cast columns, or `None` |
| `rewrite_source(source, *, dialect="postgres", schema_sql=None, import_style=None, untyped=False, json_types="typing.Any")` | Rewritten source; `json_types="typing.TypedDict"` generates `TypedDict`s for JSON columns |
| `strip_source(source, *, import_style=None)` | Stripped source, without generated `TypedDict`s |
| `fix_source(source, *, dialect="postgres", schema_sql=None, import_style=None, untyped=False, rules=None, json_types="typing.Any")` | Source with `sqlty check --fix` applied |
| `check_source(source, *, dialect="postgres", schema_sql=None, rules=None, import_style=None)` | List of diagnostics: `Diagnostic` dicts (`sqlty._lint.Diagnostic`, a `TypedDict`) with `rule`, `message`, `severity`, `line`, `start`, `end`, `fix` |
