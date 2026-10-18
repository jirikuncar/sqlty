# Unused ignore

`unused-ignore` warns by default about a comment that suppresses nothing.

## On by default

```python
# error: [unused-ignore] "'# sqlty: ignore' suppresses no diagnostic"
x = 1  # sqlty: ignore
# error: [unused-ignore] "'# sqlty: ignore[select-star]' suppresses no diagnostic"
y = 2  # sqlty: ignore[select-star]
```

## Turned off

```toml
[tool.sqlty.rules]
unused-ignore = "ignore"
```

```python
x = 1  # sqlty: ignore
# error:
```

## Bare comment that suppresses nothing

```toml
[tool.sqlty.rules]
unused-ignore = "warn"
```

```python
x = 1
# error: [unused-ignore] "'# sqlty: ignore' suppresses no diagnostic"
y = 2  # sqlty: ignore
```

## Listed rule that suppresses nothing

```toml
[tool.sqlty.rules]
unused-ignore = "error"
```

```python
from sqlty import SQL

# error: [unused-ignore] "'# sqlty: ignore[select-star]' suppresses no diagnostic"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[select-star]
```

## Used comment is not reported

```toml
[tool.sqlty.rules]
unused-ignore = "error"
```

```python
from sqlty import SQL

# error:
q = SQL[tuple[int]]("--sql\nSELECT * FROM users")  # sqlty: ignore[select-star-no-schema]
```

## Each listed rule is checked separately

```toml
[tool.sqlty.rules]
unused-ignore = "warn"
```

```python
from sqlty import SQL

# error: [unused-ignore] "'# sqlty: ignore[select-star]' suppresses no diagnostic"
q = SQL[tuple[int]]("--sql\nSELECT * FROM users")  # sqlty: ignore[select-star, select-star-no-schema]
```
