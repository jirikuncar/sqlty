# Untyped Queries

Some queries return rows sqlty can't type: `VALUES`, unparseable SQL, unknown
functions. By default rewrite leaves them as plain strings. With
`untyped = "sentinel"` (or `sqlty rewrite --untyped sentinel`) they are
wrapped as `SQL[UNTYPED](...)` instead.

## Default: left as strings

```python
q = "--sql\nVALUES (1, 2)"
# rewritten: q = "--sql\nVALUES (1, 2)"
```

## Sentinel

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nVALUES (1, 2)"
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
```

### Statements without rows

DDL and DML without `RETURNING` return no rows and stay unwrapped:

```python
ddl = "--sql\nCREATE TABLE t (id INTEGER)"
# rewritten: ddl = "--sql\nCREATE TABLE t (id INTEGER)"
```

## Checking

`UNTYPED` is a [PEP 661](https://peps.python.org/pep-0661/) sentinel, not
`Any`: mypy, pyright and ty reject using its rows as a concrete type.
`sqlty check` reports it until casts or a schema make the row type known, and
says when that has happened:

```python
from sqlty import SQL, UNTYPED

# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
unknown = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
# error: [untyped-query] "SQL[UNTYPED] can now be SQL[tuple[int]]; strip and rewrite to update"
known = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
```

## Generated stubs

The setting also applies to `.pyi` stubs generated for `@sqlty` functions
(`SQLTY_AUTO_GENERATE=1`): uninferable queries get a `SQL[UNTYPED]` overload,
and the catch-all `(query: str)` overload returns `SQL[UNTYPED]` (or the
function's `return_type_template` applied to `UNTYPED`) instead of `str`, so
a call missing from a stale stub is a type error rather than silently untyped.
