# Example: Configurable Import Style

Set `import-style` in `pyproject.toml` to control how sqlty writes the SQL wrapper.

## Configuration

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

## Supported styles

| `import-style` value               | Wrapper emitted           | Import line added              |
|------------------------------------|---------------------------|--------------------------------|
| `from sqlty import SQL` (default)  | `SQL[T](...)`             | `from sqlty import SQL`        |
| `from sqlty import SQL as Q`       | `Q[T](...)`               | `from sqlty import SQL as Q`   |
| `import sqlty`                     | `sqlty.SQL[T](...)`       | `import sqlty`                 |
| `import sqlty as sy`               | `sy.SQL[T](...)`          | `import sqlty as sy`           |

## Try it

```bash
# Rewrite uses Q[T](...) wrapper
sqlty rewrite .

# Strip removes Q[T](...) wrapper
sqlty strip .

# Check detects Q[T](...) wrappers
sqlty check .
```

## Round-trip guarantee

With the same `import-style` configuration, `strip(rewrite(source)) == source`.
