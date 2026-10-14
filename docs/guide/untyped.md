# Untyped queries

Some queries return rows whose type sqlty cannot infer: columns of tables without a schema and
`SELECT` statements sqlty cannot parse. By default `sqlty rewrite` leaves
them as plain strings, so nothing marks them as untyped:

```python
pairs = "--sql\nSELECT x, y FROM pairs"
# rewritten: pairs = "--sql\nSELECT x, y FROM pairs"
```

## The `UNTYPED` sentinel

Set `untyped = "sentinel"` to wrap them as `SQL[UNTYPED](...)` instead:

```toml
[tool.sqlty]
untyped = "sentinel"  # or: sqlty rewrite --untyped sentinel
```

```python
pairs = "--sql\nSELECT x, y FROM pairs"
users = "--sql\nSELECT id::INTEGER FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: pairs = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
# rewritten-file: users = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

`UNTYPED` is a [PEP 661](https://peps.python.org/pep-0661/) sentinel, not
`Any`. mypy, pyright and ty reject using an `UNTYPED` row as a concrete type,
so code that relies on the row's shape fails type checking until the query is
typed.

## Statements without rows

Statements that return no rows stay plain strings in both modes: DML without
`RETURNING`, DDL and transaction control. So does the `"--sql SELECT ..."`
form, which is a SQL comment.

```python
purge = "--sql\nDELETE FROM users WHERE id = 1"
table = "--sql\nCREATE TABLE users (id INTEGER)"
deleted = "--sql\nDELETE FROM users RETURNING id::INTEGER"
# rewritten: purge = "--sql\nDELETE FROM users WHERE id = 1"
# rewritten: table = "--sql\nCREATE TABLE users (id INTEGER)"
# rewritten: deleted = SQL[tuple[int]]("--sql\nDELETE FROM users RETURNING id::INTEGER")
```

`sqlty check` reports nothing for them:
[`stale-rewrite`](../reference/rules.md#stale-rewrite) covers only strings
that `rewrite` would wrap.

## The `untyped-query` rule

`sqlty check` reports every `SQL[UNTYPED]` query as
[`untyped-query`](../reference/rules.md#untyped-query) (warning by default).
Once casts or a schema make the type known, the message names it:

```python
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
pairs = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

```python
# error: [untyped-query] "SQL[UNTYPED] can now be SQL[tuple[int]]; run sqlty check --fix"
users = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
```

`sqlty rewrite` does not replace an existing wrapper; `sqlty check --fix`
does. See [Editing queries](writing-queries.md#editing-queries).
