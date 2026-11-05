# Rewrite Idempotency

## Already rewritten source is unchanged

```python
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Already rewritten with multiple types

```python
q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Manual SQL[CustomType] without --sql is not rewritten

User-defined `SQL[T]` wrappers without `--sql` prefix are left untouched:

```python
q = SQL[User]("SELECT id, name FROM users")
# rewritten: q = SQL[User]("SELECT id, name FROM users")
```

## Manual SQL[CustomType] with --sql is not double-wrapped

Already-wrapped `--sql` strings are detected by the existing `SQL[` wrapper
and skipped regardless of the type parameter:

```python
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten: q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Multiline SQL[CustomType] is not rewritten

```python
q = SQL[User]("""--sql
SELECT id::INTEGER, name::TEXT FROM users""")
# rewritten: q = SQL[User]("""--sql
# rewritten: SELECT id::INTEGER, name::TEXT FROM users""")
```

## Non-SQL code is unchanged

```python
x = 42
# rewritten: x = 42
```

## An untyped SQL(...) call gets its row type, not a second wrapper

`rewrite` types a `--sql` string already passed to sqlty's `SQL`, under any
name it is imported as:

```python
from sqlty import SQL
from sqlty import SQL as Query
import sqlty

q1 = SQL("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
q2 = Query("--sql\nSELECT id::INTEGER FROM users")
q3 = sqlty.SQL(
    "--sql\nSELECT name::TEXT FROM users",
)
# rewritten: from sqlty import SQL as Query
# rewritten: import sqlty
# rewritten:
# rewritten: q1 = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten: q2 = Query[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten: q3 = sqlty.SQL[tuple[str]](
# rewritten:     "--sql\nSELECT name::TEXT FROM users",
# rewritten: )
```

## An aliased SQL[T] call is not wrapped again

```python
from sqlty import SQL as Query

q = Query[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten: from sqlty import SQL as Query
# rewritten:
# rewritten: q = Query[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## A SQL(...) call of another library is not typed

```python
from psycopg.sql import SQL

q = SQL("--sql\nSELECT id::INTEGER FROM users")
# rewritten: from psycopg.sql import SQL
# rewritten:
# rewritten: q = SQL("--sql\nSELECT id::INTEGER FROM users")
```
