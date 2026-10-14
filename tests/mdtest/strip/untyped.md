# Strip: UNTYPED Sentinel

`strip` removes `SQL[UNTYPED](...)` wrappers like any other, together with
the `UNTYPED` import once nothing references it.

## Strip removes SQL[UNTYPED] and its imports

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
# stripped: q = "--sql\nSELECT x, y FROM pairs"
```

## Strip keeps UNTYPED import while still referenced

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
assert row_form(q) is UNTYPED
# stripped: from sqlty import UNTYPED
# stripped: q = "--sql\nSELECT x, y FROM pairs"
# stripped: assert row_form(q) is UNTYPED
```

## Round trip: rewrite then strip

```toml
[tool.sqlty]
untyped = "sentinel"
```

### Rewrite

```python
q = "--sql\nSELECT x, y FROM pairs"
# rewritten: q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

### Strip

```python
q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
# stripped: q = "--sql\nSELECT x, y FROM pairs"
```

## Strip with module import

```toml
[tool.sqlty]
import-style = "import sqlty"
```

```python
import sqlty
q = sqlty.SQL[sqlty.UNTYPED]("--sql\nSELECT x, y FROM pairs")
# stripped: q = "--sql\nSELECT x, y FROM pairs"
```
