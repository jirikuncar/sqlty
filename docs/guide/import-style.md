# Import Style

`import-style` sets the import line rewrite adds and how the wrapper is
spelled. Strip uses the same setting, so the round trip stays exact. Runnable
project: [`examples/05_import_style/`](../../examples/05_import_style/).

| `import-style` | Wrapper |
|----------------|---------|
| `from sqlty import SQL` (default) | `SQL[T](...)` |
| `from sqlty import SQL as Q` | `Q[T](...)` |
| `import sqlty` | `sqlty.SQL[T](...)` |
| `import sqlty as sy` | `sy.SQL[T](...)` |

## Aliased name

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten-file: from sqlty import SQL as Q
# rewritten-file: q = Q[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Module import

```toml
[tool.sqlty]
import-style = "import sqlty"
```

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten-file: import sqlty
# rewritten-file: q = sqlty.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Module alias

```toml
[tool.sqlty]
import-style = "import sqlty as sy"
```

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten-file: import sqlty as sy
# rewritten-file: q = sy.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

### Strip

```python
import sqlty as sy

q = sy.SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

### Check

Checks follow the configured spelling:

```python
import sqlty as sy

# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = sy.SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
```
