# Strip: UNTYPED Sentinel

`strip` removes `SQL[UNTYPED](...)` wrappers like any other, together with
the `UNTYPED` import once nothing references it.

## Strip removes SQL[UNTYPED] and its imports

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
# stripped: q = "--sql\nVALUES (1, 2)"
```

## Strip keeps UNTYPED import while still referenced

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
assert row_form(q) is UNTYPED
# stripped: from sqlty import UNTYPED
# stripped: q = "--sql\nVALUES (1, 2)"
# stripped: assert row_form(q) is UNTYPED
```

## Round trip: rewrite then strip

```toml
[sqlty]
untyped = "sentinel"
```

```python
q = "--sql\nVALUES (1, 2)"
# rewritten: q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
```

```python
q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
# stripped: q = "--sql\nVALUES (1, 2)"
```

## Strip with module import

```toml
[sqlty]
import-style = "import sqlty"
```

```python
import sqlty
q = sqlty.SQL[sqlty.UNTYPED]("--sql\nVALUES (1, 2)")
# stripped: q = "--sql\nVALUES (1, 2)"
```
