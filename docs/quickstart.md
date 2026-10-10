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

## 4. Edit with strip and rewrite

To change a query, strip the wrappers, edit, and rewrite:

```bash
sqlty strip .     # SQL[T]("--sql\n...") -> "--sql\n..."
# edit the SQL
sqlty rewrite .   # "--sql\n..." -> SQL[T]("--sql\n...")
```

```python
user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# stripped: user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
```

Neither command changes the string contents, only the wrapper and the
`from sqlty import SQL` import. For a file whose `--sql` strings are all
plain, `strip(rewrite(x)) == x`. `strip` removes every wrapper, including a
hand-written `SQL[User]`, and the sqlty import even if it was there before;
see [Migration](migration.md#pitfalls).

## 5. Name the columns

Replace `tuple[...]` with a `TypedDict`, `NamedTuple`, dataclass or Pydantic
model, and `sqlty check` validates field count, types and names:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

user_query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

See [Row types](guide/row-types.md).

## 6. Run it before every commit

Add sqlty to [pre-commit](https://pre-commit.com) as local hooks:

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-rewrite
        name: sqlty rewrite
        entry: sqlty rewrite .
        language: system
        pass_filenames: false
        types: [python]
      - id: sqlty-check
        name: sqlty check
        entry: sqlty check .
        language: system
        pass_filenames: false
        types: [python]
```

[CI](ci.md) shows how to fail a build on unwrapped or stale queries.

## Next steps

- Runnable version of this page:
  [`examples/01_quick_start/`](https://github.com/jirikuncar/sqlty/tree/main/examples/01_quick_start).
- [psycopg](guide/psycopg.md): typed rows from `SQL[T]` at runtime.
- [Use cases](use-cases.md): where sqlty fits.
