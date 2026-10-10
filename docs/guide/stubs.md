# Generated Stubs

If your code sends queries through a helper function, such as
`sql("SELECT ...")` or `fetch_all("SELECT ...")`, sqlty can type the helper
instead of each string. It writes a `.pyi` stub next to the helper's module
with one `@overload` per query literal, so a type checker sees
`sql("SELECT id, name FROM users")` return `SQL[tuple[int, str]]`.

Stubs are generated at runtime: run your code (or its tests) with
`SQLTY_AUTO_GENERATE=1` and every call that executes is recorded. There is no
CLI command for stub generation.

## Set up a helper

1. Decorate the helper with `@sqlty`. Pass `schema=` to infer types from a
   schema file; the path is relative to the helper's module.

    ```python
    # db.py
    from typing import Any

    from sqlty import SQL, sqlty


    @sqlty(schema="schema.sql")
    def sql(query: str) -> SQL[Any]:
        return SQL(query)
    ```

2. Register it in `pyproject.toml`. `import_path` is the module path plus the
   function name; `query_param` names the parameter that holds the query.

    ```toml
    [[tool.sqlty.functions]]
    import_path = "db.sql"
    query_param = "query"
    ```

3. Run the code that calls the helper:

    ```python
    # app.py
    from db import sql

    users = sql("SELECT id, name FROM users")
    recent = sql("SELECT created_at FROM users ORDER BY created_at DESC LIMIT 1")
    ```

    ```bash
    SQLTY_AUTO_GENERATE=1 python app.py
    ```

    `1`, `true` and `yes` (any case) enable generation. When the process
    exits, sqlty writes `db.pyi`:

    ```python
    import datetime
    from typing import Literal, overload
    from sqlty import SQL


    @overload
    def sql(query: Literal['SELECT created_at FROM users ORDER BY created_at DESC LIMIT 1']) -> SQL[tuple[datetime.datetime]]:
        ...

    @overload
    def sql(query: Literal['SELECT id, name FROM users']) -> SQL[tuple[int, str]]:
        ...

    @overload
    def sql(query: str) -> str:
        ...
    ```

A type checker now reveals `sql("SELECT id, name FROM users")` as
`SQL[tuple[int, str]]`. A query string without its own overload falls through
to the last, catch-all overload.

Queries passed to a helper do not use the `--sql` prefix. The literal in the
overload must match the call exactly, so a reformatted query needs a new run.

The stub replaces `db.py` for type checkers, which then see only what the
stub declares. Keep helpers in a module of their own.

If a decorated function is not registered, `SQLTY_AUTO_GENERATE=1` emits a
warning with the `[[tool.sqlty.functions]]` entry to add, and no stub is
written. For a script run directly (`python db.py`), use the file name or
`__main__` as the module: `import_path = "db.sql"` or `"__main__.sql"`.

Tests covering this flow:
[`tests/generator/test_auto_generate.py`](https://github.com/jirikuncar/sqlty/blob/main/tests/generator/test_auto_generate.py).

## Schemas

`@sqlty(schema="schema.sql")` is the only schema source for stub generation.
Without it, columns need casts. The `schema` key of `[tool.sqlty]` and of
`[[tool.sqlty.functions]]` are not used for stubs.

## Return types

By default each overload returns `SQL[T]`. `return_type_template` wraps the
inferred type differently; `{sqlty}` stands for it:

```toml
[[tool.sqlty.functions]]
import_path = "db.fetch_all"
query_param = "query"
return_type_template = "list[{sqlty}]"
```

A call `fetch_all("SELECT COUNT(*)::INTEGER FROM users")` then gets the
overload `-> list[tuple[int]]`. The template is written into the stub as is,
and runtime generation adds only the imports for `SQL`, `datetime`, `uuid`
and `UNTYPED`, so use names the stub can resolve without other imports.

A query sqlty cannot type gets an `SQL[Any]` overload (the template applied
to `Any`), and a warning naming the query is logged.

## Untyped calls

With [`untyped = "sentinel"`](untyped.md) in `[tool.sqlty]`:

- A row-returning query sqlty cannot type, such as `VALUES (1, 2)`, gets an
  overload returning `SQL[UNTYPED]` (or the template applied to `UNTYPED`).
  Statements without rows still get `SQL[Any]`.
- The catch-all `(query: str)` overload returns `SQL[UNTYPED]` (or the
  template applied to `UNTYPED`) instead of `str`. A call missing from a
  stale stub is then a type error instead of silently untyped.

```python
@overload
def sql(query: Literal['SELECT id::INTEGER FROM users']) -> SQL[tuple[int]]:  # type: ignore[overload-overlap]
    ...

@overload
def sql(query: Literal['VALUES (1, 2)']) -> SQL[UNTYPED]:
    ...

@overload
def sql(query: str) -> SQL[UNTYPED]:
    ...
```

Typed overloads overlap the `SQL[UNTYPED]` catch-all with an incompatible
return type, so each carries its own `# type: ignore[overload-overlap]`.
Other overlaps in the stub are still reported. pyright reports only the first
such overlap per catch-all, so with `reportUnnecessaryTypeIgnoreComment`
enabled it flags the remaining ignores; with
`enableTypeIgnoreComments = false` it reports the overlaps instead.

Tests:
[`tests/generator/test_untyped_stubs.py`](https://github.com/jirikuncar/sqlty/blob/main/tests/generator/test_untyped_stubs.py).

## Updating stubs

Each run updates the existing stub incrementally:

- Overloads for new queries are added.
- An overload whose return type changed is replaced, including one you edited
  by hand. Switching `untyped` mode therefore rewrites `SQL[Any]` and
  `SQL[UNTYPED]` overloads on the next run.
- Overloads are never removed, even when their query is gone. Delete the
  `.pyi` file and rerun to start fresh.
- Overloads are sorted by query text, so the output is stable.

Only calls that execute during the run are recorded. Run your test suite with
`SQLTY_AUTO_GENERATE=1` to cover the most queries.

## Configuration keys

| Key in `[[tool.sqlty.functions]]` | Used by runtime generation |
|------|------|
| `import_path` (required) | Yes: matches the decorated function |
| `query_param` (required) | Required to register; the query is the first argument |
| `return_type_template` | Yes |
| `imports` | No |
| `schema` | No; use `@sqlty(schema=...)` |
| `return_type_param` | No |
| `self_type_template` | No; methods are not supported |

See [Configuration](../reference/configuration.md#functions).

## Limitations

- With `SQLTY_AUTO_GENERATE=1`, the decorator wraps a registered helper in a
  function that takes exactly one argument, `query`. Helpers with more
  parameters fail when called.
- Only module-level functions are supported, not methods.
- Stubs are written with `fcntl` file locks, so generation works on POSIX
  systems only.
