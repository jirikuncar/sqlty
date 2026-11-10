# Configurable Import Style

The `import-style` config controls the import line and wrapper prefix.

## Default: from sqlty import SQL

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: from sqlty import NoParams
# rewritten: q = SQL[tuple[int], NoParams]("--sql\nSELECT id::INTEGER FROM users")
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import NoParams
# rewritten-file: q = SQL[tuple[int], NoParams]("--sql\nSELECT id::INTEGER FROM users")
```

## Aliased import: from sqlty import SQL as Q

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: from sqlty import NoParams
# rewritten: q = Q[tuple[int], NoParams]("--sql\nSELECT id::INTEGER FROM users")
# rewritten-file: from sqlty import SQL as Q
# rewritten-file: from sqlty import NoParams
# rewritten-file: q = Q[tuple[int], NoParams]("--sql\nSELECT id::INTEGER FROM users")
```

## Module import: import sqlty

```toml
[tool.sqlty]
import-style = "import sqlty"
```

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = sqlty.SQL[tuple[int], sqlty.NoParams]("--sql\nSELECT id::INTEGER FROM users")
# rewritten-file: import sqlty
# rewritten-file: q = sqlty.SQL[tuple[int], sqlty.NoParams]("--sql\nSELECT id::INTEGER FROM users")
```

## Module alias: import sqlty as sy

```toml
[tool.sqlty]
import-style = "import sqlty as sy"
```

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = sy.SQL[tuple[int], sy.NoParams]("--sql\nSELECT id::INTEGER FROM users")
# rewritten-file: import sqlty as sy
# rewritten-file: q = sy.SQL[tuple[int], sy.NoParams]("--sql\nSELECT id::INTEGER FROM users")
```

## Aliased import with multiple columns

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as SQLT"
```

```python
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# rewritten: from sqlty import NoParams
# rewritten: q = SQLT[tuple[int, str], NoParams]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Module alias with datetime type

```toml
[tool.sqlty]
import-style = "import sqlty as y"
```

```python
q = "--sql\nSELECT created_at::TIMESTAMP FROM events"
# rewritten: q = y.SQL[tuple[datetime.datetime], y.NoParams]("--sql\nSELECT created_at::TIMESTAMP FROM events")
# rewritten-file: import datetime
# rewritten-file: import sqlty as y
# rewritten-file: q = y.SQL[tuple[datetime.datetime], y.NoParams]("--sql\nSELECT created_at::TIMESTAMP FROM events")
```

## Idempotent rewrite with alias

Already-wrapped source using the configured alias is unchanged:

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten: from sqlty import NoParams
# rewritten: q = Q[tuple[int], NoParams]("--sql\nSELECT 1::INTEGER")
```

## Idempotent rewrite with module import

```toml
[tool.sqlty]
import-style = "import sqlty"
```

```python
q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten: q = sqlty.SQL[tuple[int], sqlty.NoParams]("--sql\nSELECT 1::INTEGER")
```
