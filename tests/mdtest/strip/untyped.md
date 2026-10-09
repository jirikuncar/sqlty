# Strip: UNTYPED Sentinel

`strip` removes `SQL[UNTYPED](...)` wrappers like any other, together with
the `UNTYPED` import once nothing references it.

## Strip removes SQL[UNTYPED] and its imports

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nDELETE FROM users")
# stripped: q = "--sql\nDELETE FROM users"
```

## Strip keeps UNTYPED import while still referenced

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nDELETE FROM users")
assert row_form(q) is UNTYPED
# stripped: from sqlty import UNTYPED
# stripped: q = "--sql\nDELETE FROM users"
# stripped: assert row_form(q) is UNTYPED
```

## Round trip: rewrite then strip

```toml
[sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nDELETE FROM users"
# rewritten: q = SQL[UNTYPED]("--sql\nDELETE FROM users")
```

```python
q = SQL[UNTYPED]("--sql\nDELETE FROM users")
# stripped: q = "--sql\nDELETE FROM users"
```

## Strip with module import

```toml
[sqlty]
import-style = "import sqlty"
```

```python
import sqlty
q = sqlty.SQL[sqlty.UNTYPED]("--sql\nDELETE FROM users")
# stripped: q = "--sql\nDELETE FROM users"
```
