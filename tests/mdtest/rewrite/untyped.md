# Rewrite: UNTYPED Sentinel

Queries without an inferable row type (statements without `RETURNING`,
DDL, unparseable SQL) are left as plain strings by default. With
`untyped = "sentinel"` they are wrapped as `SQL[UNTYPED](...)`, so type
checkers reject treating their rows as a concrete type.

## Default: uninferable query is left unwrapped

```python
q = "--sql\nDELETE FROM users WHERE id = 1"
# rewritten: q = "--sql\nDELETE FROM users WHERE id = 1"
```

## Sentinel: uninferable query is wrapped as SQL[UNTYPED]

```toml
[sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nDELETE FROM users WHERE id = 1"
# rewritten: q = SQL[UNTYPED]("--sql\nDELETE FROM users WHERE id = 1")
```

## Sentinel: imports UNTYPED next to SQL

```toml
[sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nCREATE TABLE users (id INTEGER)"
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = SQL[UNTYPED]("--sql\nCREATE TABLE users (id INTEGER)")
```

## Sentinel: inferable queries keep their row type

```toml
[sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nSELECT id::INTEGER FROM users"
d = "--sql\nDELETE FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten-file: d = SQL[UNTYPED]("--sql\nDELETE FROM users")
```

## Sentinel: RETURNING makes the row type inferable

```toml
[sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nDELETE FROM users RETURNING id::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\nDELETE FROM users RETURNING id::INTEGER")
```

## Sentinel: existing SQL[UNTYPED] wrapper is left alone

```toml
[sqlty]
untyped = "sentinel"
```

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nDELETE FROM users")
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = SQL[UNTYPED]("--sql\nDELETE FROM users")
```

## Sentinel with aliased import

```toml
[sqlty]
untyped = "sentinel"
import-style = "from sqlty import SQL as Q"
```

```python
q = "--sql\nDELETE FROM users"
# rewritten-file: from sqlty import SQL as Q
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: q = Q[UNTYPED]("--sql\nDELETE FROM users")
```

## Sentinel with module import

```toml
[sqlty]
untyped = "sentinel"
import-style = "import sqlty"
```

```python
q = "--sql\nDELETE FROM users"
# rewritten-file: import sqlty
# rewritten-file: q = sqlty.SQL[sqlty.UNTYPED]("--sql\nDELETE FROM users")
```

## Sentinel with module alias

```toml
[sqlty]
untyped = "sentinel"
import-style = "import sqlty as sy"
```

```python
q = "--sql\nDELETE FROM users"
# rewritten-file: import sqlty as sy
# rewritten-file: q = sy.SQL[sy.UNTYPED]("--sql\nDELETE FROM users")
```
