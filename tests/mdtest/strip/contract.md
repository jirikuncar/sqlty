# Strip Contract

## `strip_source`

### Handles space format

```python
from sqlty import SQL
q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
# stripped: q = "--sql SELECT 1::INTEGER"
```

### Returns str

```python
x = 1
# stripped: x = 1
```

## Untyped sentinel

### Strip keeps import still in use

```python
from sqlty import SQL
from sqlty import UNTYPED
q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
UNTYPED
# stripped: from sqlty import UNTYPED
# stripped: q = "--sql\nSELECT x, y FROM pairs"
# stripped: UNTYPED
```

## Strip_source removes SQL[T](...) wrappers

```python
from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```
