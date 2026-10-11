# Quickstart

This page takes one file from plain SQL strings to typed, checked queries.
Install sqlty first ([Installation](installation.md)).

## 1. Mark queries with `--sql`

Start each query string with `--sql` and a newline, and cast the columns
whose types you want:

```python
# queries.py
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
count_query = "--sql\nSELECT COUNT(*)::INTEGER FROM users"
```

[Writing queries](guide/writing-queries.md) covers the prefix, casts and the
type mapping. With a [schema file](guide/schema-inference.md) you can drop
the casts.

## 2. Rewrite

```bash
sqlty rewrite .
```

`sqlty rewrite` wraps each query in `SQL[T](...)` with the inferred row type
and adds the import. `queries.py` afterwards:

```python
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
count_query = "--sql\nSELECT COUNT(*)::INTEGER FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten-file: count_query = SQL[tuple[int]]("--sql\nSELECT COUNT(*)::INTEGER FROM users")
```

`SQL[T]` is a `str` subclass, so the queries still work anywhere a string
does. Editors and type checkers now see `T`.

## 3. Check

```bash
sqlty check .
```

`sqlty check` prints nothing when everything matches. Edit the first query
to select `email::TEXT` as well, and it reports the drift:

```
queries.py:2: [stale-type] SQL[tuple[int, str]] should be SQL[tuple[int, str, str]]
Found 1 warning(s)
```

Warnings leave the exit code at 0; errors such as a
[row type](guide/row-types.md) mismatch exit with 1. The
[rules reference](reference/rules.md) lists every diagnostic.

## 4. Name the columns

Replace `tuple[...]` with a `TypedDict`, `NamedTuple`, dataclass or Pydantic
model, and `sqlty check` validates field count, types and names:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

user_query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# check:
```

The empty `# check:` line means `sqlty check` reports nothing for this file.
See [Row types](guide/row-types.md).

## 5. Edit queries

`sqlty rewrite` does not touch existing wrappers. After you change a query:

- **Edit in place.** Change the SQL inside `SQL[T](...)` and let
  `sqlty check` report a stale `T`, as in step 3. Use this for files with
  row types like `User`.
- **Strip and rewrite.** For files that only have `SQL[tuple[...]]`, run
  `sqlty strip`, edit, and `sqlty rewrite` to regenerate every type.

!!! warning
    `sqlty strip` also removes hand-written row types such as `SQL[User]`.
    See [Editing queries](guide/writing-queries.md#editing-queries).

## 6. Run it before every commit

Add `sqlty rewrite` and `sqlty check` as
[pre-commit hooks](recipes/ci.md#pre-commit). [CI and pre-commit](recipes/ci.md)
also shows how to fail a build on unwrapped or stale queries.

## Next steps

- Runnable version of this page:
  [`examples/01_quick_start/`](https://github.com/jirikuncar/sqlty/tree/main/examples/01_quick_start).
- [psycopg](guide/psycopg.md): typed rows from `SQL[T]` at runtime.
- [Use cases](use-cases.md): where sqlty fits.
- [Example projects](index.md#example-projects): all runnable examples.
