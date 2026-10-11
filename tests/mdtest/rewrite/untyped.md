# Rewrite: UNTYPED Sentinel

Row-returning queries whose row type can't be inferred (`VALUES`,
unparseable SQL) are left as plain strings by default. With
`untyped = "sentinel"` they are wrapped as `SQL[UNTYPED](...)`, so type
checkers reject treating their rows as a concrete type. Statements that
return no rows, and the `"--sql SELECT ..."` comment form, stay unwrapped.

## Default: uninferable query is left unwrapped

```python
q = "--sql\nVALUES (1, 2)"
# rewritten: q = "--sql\nVALUES (1, 2)"
```

## Sentinel: uninferable query is wrapped as SQL[UNTYPED]

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nVALUES (1, 2)"
# rewritten: q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
```

## Sentinel: DML without RETURNING returns no rows

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nDELETE FROM users WHERE id = 1"
i = "--sql\nINSERT INTO users (id) VALUES (1)"
u = "--sql\nUPDATE users SET name = 'x'"
# rewritten: q = "--sql\nDELETE FROM users WHERE id = 1"
# rewritten: i = "--sql\nINSERT INTO users (id) VALUES (1)"
# rewritten: u = "--sql\nUPDATE users SET name = 'x'"
```

## Sentinel: DDL and transaction control return no rows

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
c = "--sql\nCREATE TABLE users (id INTEGER)"
d = "--sql\nDROP TABLE users"
b = "--sql\nBEGIN"
# rewritten: c = "--sql\nCREATE TABLE users (id INTEGER)"
# rewritten: d = "--sql\nDROP TABLE users"
# rewritten: b = "--sql\nBEGIN"
```

## Sentinel: the space form is a SQL comment, not a query

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql VALUES (1, 2)"
# rewritten: q = "--sql VALUES (1, 2)"
```

## Sentinel: empty query is left alone

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\n"
# rewritten: q = "--sql\n"
```

## Sentinel: imports UNTYPED next to SQL

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nSELECT FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = SQL[UNTYPED]("--sql\nSELECT FROM users")
```

## Sentinel: inferable queries keep their row type

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nSELECT id::INTEGER FROM users"
d = "--sql\nSELEC id FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten-file: d = SQL[UNTYPED]("--sql\nSELEC id FROM users")
```

## Sentinel: RETURNING makes the row type inferable

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nDELETE FROM users RETURNING id::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\nDELETE FROM users RETURNING id::INTEGER")
```

## Sentinel: existing SQL[UNTYPED] wrapper is left alone

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
```

## Sentinel with aliased import

```toml
[tool.sqlty]
untyped = "sentinel"
import-style = "from sqlty import SQL as Q"
```

```python
q = "--sql\nVALUES (1, 2)"
# rewritten-file: from sqlty import SQL as Q
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = Q[UNTYPED]("--sql\nVALUES (1, 2)")
```

## Sentinel with module import

```toml
[tool.sqlty]
untyped = "sentinel"
import-style = "import sqlty"
```

```python
q = "--sql\nVALUES (1, 2)"
# rewritten-file: import sqlty
# rewritten-file: q = sqlty.SQL[sqlty.UNTYPED]("--sql\nVALUES (1, 2)")
```

## Sentinel with module alias

```toml
[tool.sqlty]
untyped = "sentinel"
import-style = "import sqlty as sy"
```

```python
q = "--sql\nVALUES (1, 2)"
# rewritten-file: import sqlty as sy
# rewritten-file: q = sy.SQL[sy.UNTYPED]("--sql\nVALUES (1, 2)")
```
