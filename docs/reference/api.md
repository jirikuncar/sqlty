# Python API

## `sqlty.SQL[T]`

A generic `str` subclass carrying the row type `T`. `sqlty rewrite` writes
`SQL[T](...)` calls; at runtime the value is the original string.

```python
from sqlty import SQL

query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
assert isinstance(query, str)
# error:
```

## `sqlty.UNTYPED`

The row type of queries sqlty can't type, written by
`sqlty rewrite --untyped sentinel`. See [untyped queries](../guide/untyped.md).

## `sqlty.row_form(query[, default])`

Returns the `T` of a query built as `SQL[T](...)`, typed as `TypeForm[T]`.
Plain strings, `SQL("...")`, annotation-only `q: SQL[T] = "..."` and stripped
sources carry no runtime type: they raise `TypeError`, or return `default`.

```python
from sqlty import SQL, row_form

row_form(SQL[tuple[int, str]]("--sql\nSELECT 1::INTEGER, 'a'::TEXT"))  # tuple[int, str]
row_form(SQL("--sql\nSELECT 1"), None)  # None
```

## `sqlty.core`

The engine behind the CLI, backed by the Rust extension when it is compiled
and by pure Python otherwise (`SQLTY_LIB=py|rs` forces one). Every function
takes `dialect="postgres"` and, where types are inferred, `schema_sql` with
`CREATE TABLE` statements.

| Function | Returns |
|----------|---------|
| `infer_type(sql)` | row type such as `"tuple[int, str]"`, or `None` |
| `expand_star(sql)` | the query with `*` replaced by cast columns, or `None` |
| `rewrite_source(source, *, import_style=None, untyped=False)` | source with queries wrapped |
| `strip_source(source, *, import_style=None)` | source with wrappers removed |
| `fix_source(source, *, import_style=None, untyped=False, rules=None)` | source with `sqlty check --fix` applied |
| `check_source(source, *, rules=None, import_style=None)` | diagnostics: `rule`, `message`, `severity`, `line` |

The [guide](../guide/quick-start.md) and [rules](../rules.md) pages are
executed against these functions on both backends.

## Drivers

- [psycopg 3](../guide/psycopg.md): `sqlty.drivers.psycopg.execute`,
  `scalar` and `row_factory`.
