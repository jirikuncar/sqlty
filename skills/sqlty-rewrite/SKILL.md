---
name: sqlty-rewrite
description: Wrap --sql query strings as SQL[T] with sqlty rewrite, update or remove the wrappers, and keep them in sync after editing a query. Use when writing a new SQL query in Python, editing the SQL inside an SQL[T] wrapper, committing or removing wrappers (sqlty rewrite, sqlty strip, --check, --diff), choosing an import-style, wrapping untyped queries as SQL[UNTYPED], or using the sqlty magic codec (a "coding sqlty" source encoding) instead of committed wrappers.
---

# sqlty rewrite

`sqlty rewrite` wraps each `--sql` string as `SQL[T]("--sql\n...")`, with `T`
the inferred row type, and adds the import. `sqlty strip` removes the wrappers.
Neither changes the string itself.

Run `sqlty version`: this skill may be newer or older than the installed sqlty.
When it and `sqlty --help` disagree, trust the command.

## Write a new query

1. Start the string with `--sql` and a newline.
2. Run `sqlty rewrite <file>`.
3. Run `sqlty check <file>`. It must report nothing.

```python
user_id = 1
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = %s"
# rewritten: user_id = 1
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = %s")
```

## What `rewrite` leaves alone

| String | Why |
|--------|-----|
| `INSERT`, `UPDATE`, `DELETE` without `RETURNING`; DDL | returns no rows |
| SQL the parser rejects | no type to infer; `sqlty check` reports it |
| a column sqlty can't type | an unknown table or function, `SELECT *` without a schema |
| f-strings, `+`, `%` | not a literal; pass values as parameters |

Make the column typeable: cast it (`x::INTEGER`, `CAST(x AS INTEGER)`) or add the
table to the schema. Don't hand-write a guessed `T`.

```python
unknown = "--sql\nSELECT x FROM t"
typed = "--sql\nSELECT x::INTEGER FROM t"
# rewritten: unknown = "--sql\nSELECT x FROM t"
# rewritten: typed = SQL[tuple[int]]("--sql\nSELECT x::INTEGER FROM t")
```

### Mark untyped queries

With `untyped = "sentinel"`, a query whose row type is unknown becomes
`SQL[UNTYPED]`, so it is visible and `sqlty check --fix` types it once it can:

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
pairs = "--sql\nSELECT x, y FROM pairs"
# rewritten: pairs = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

`sqlty rewrite --untyped sentinel` does the same for one run.

## Edit a wrapped query

`rewrite` never replaces an existing wrapper. After changing the SQL inside
`SQL[T](...)`, run `sqlty check --fix`: it rewrites every stale `SQL[tuple[...]]`
and `SQL[UNTYPED]`, and leaves named row types for you to update.

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str, str]]("--sql\nSELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
```

Don't strip and rewrite a file to update it: `strip` removes every wrapper,
`SQL[User]` included, and the next `rewrite` writes `SQL[tuple[...]]`.

```python
user = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# stripped: user = "--sql\nSELECT id::INTEGER FROM users"
```

## Check without writing

| Command | Exits 1 when |
|---------|--------------|
| `sqlty rewrite --check .` | a file would be rewritten |
| `sqlty rewrite --diff .` | a file would be rewritten; prints the diff |
| `sqlty strip --check .` | a file still has wrappers |

For a file whose `--sql` strings are all plain, `strip(rewrite(x)) == x`.

## Import style

`import-style` changes the import line and the wrapper name:

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
query = "--sql\nSELECT id::INTEGER FROM users"
# rewritten-file: from sqlty import SQL as Q
# rewritten-file: query = Q[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## The magic codec

A file whose first or second line is `# coding: sqlty` is rewritten when Python
reads it, so it runs with wrappers that aren't on disk. Type checkers read the
file on disk, so they don't see the types: use it for scripts and notebooks,
and `sqlty rewrite` for code you commit.

```python
# coding: sqlty

query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

`SyntaxError: unknown encoding: sqlty` means the codec isn't registered: in an
editable install, `import sqlty.magic` before the first such module.

## Reference

- [Writing queries](https://jirikuncar.github.io/sqlty/guide/writing-queries/)
- [Untyped queries](https://jirikuncar.github.io/sqlty/guide/untyped/)
- [Magic codec](https://jirikuncar.github.io/sqlty/guide/magic-codec/)
- [CLI reference](https://jirikuncar.github.io/sqlty/reference/cli/)
