# Strip with Configurable Import Style

## Strip aliased import

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip module import

```toml
[tool.sqlty]
import-style = "import sqlty"
```

```python
q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip module alias

```toml
[tool.sqlty]
import-style = "import sqlty as sy"
```

```python
q = sy.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip aliased import removes import line

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
from sqlty import SQL as Q
q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip module import removes import line

```toml
[tool.sqlty]
import-style = "import sqlty"
```

```python
import sqlty
q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip module alias removes import line

```toml
[tool.sqlty]
import-style = "import sqlty as sy"
```

```python
import sqlty as sy
q = sy.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Round-trip with aliased import

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

### Rewrite

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = Q[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

### Strip

```python
q = Q[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

## Round-trip with module import

```toml
[tool.sqlty]
import-style = "import sqlty"
```

### Rewrite

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = sqlty.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

### Strip

```python
q = sqlty.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

## Round-trip with module alias

```toml
[tool.sqlty]
import-style = "import sqlty as y"
```

### Rewrite

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = y.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

### Strip

```python
q = y.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

## Non-managed SQL[T] with alias is not stripped

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
q = Q[int]("SELECT 1")
# stripped: q = Q[int]("SELECT 1")
```
