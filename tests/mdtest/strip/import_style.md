# Strip with Configurable Import Style

## Strip aliased import

```toml
[sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip module import

```toml
[sqlty]
import-style = "import sqlty"
```

```python
q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip module alias

```toml
[sqlty]
import-style = "import sqlty as sy"
```

```python
q = sy.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip aliased import removes import line

```toml
[sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
from sqlty import SQL as Q
q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip module import removes import line

```toml
[sqlty]
import-style = "import sqlty"
```

```python
import sqlty
q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Strip module alias removes import line

```toml
[sqlty]
import-style = "import sqlty as sy"
```

```python
import sqlty as sy
q = sy.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Round-trip with aliased import

```toml
[sqlty]
import-style = "from sqlty import SQL as Q"
```

Rewrite produces aliased wrapper:

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = Q[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

Strip removes aliased wrapper:

```python
q = Q[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

## Round-trip with module import

```toml
[sqlty]
import-style = "import sqlty"
```

Rewrite produces module-qualified wrapper:

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = sqlty.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

Strip removes module-qualified wrapper:

```python
q = sqlty.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

## Round-trip with module alias

```toml
[sqlty]
import-style = "import sqlty as y"
```

Rewrite produces aliased module wrapper:

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = y.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

Strip removes aliased module wrapper:

```python
q = y.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

## Non-managed SQL[T] with alias is not stripped

```toml
[sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
q = Q[int]("SELECT 1")
# stripped: q = Q[int]("SELECT 1")
```
