# Wrapper Arguments

A wrapper's argument runs to its matching `)`: every implicitly concatenated
string, comments and a trailing comma included. `check`, `check --fix`,
`rewrite` and `strip` all read it as one query, so their output stays valid
Python.

## Implicitly concatenated strings

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER "
                    "AS n")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER "
# fixed:                     "AS n")
```

### Unwrapped

```python
q = ("--sql\nSELECT 1::INTEGER "
     "AS n")
# rewritten: q = (SQL[tuple[int]]("--sql\nSELECT 1::INTEGER "
# rewritten:      "AS n"))
```

### Stripped

```python
from sqlty import SQL

q = (SQL[tuple[int]]("--sql\nSELECT 1::INTEGER "
                     "AS n"))
r = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER " "AS n")
# stripped: q = ("--sql\nSELECT 1::INTEGER "
# stripped:                      "AS n")
# stripped: r = "--sql\nSELECT 1::INTEGER " "AS n"
```

### Kept when the strings need the call's parentheses

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER "
                    "AS n")
# stripped: from sqlty import SQL
# stripped:
# stripped: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER "
# stripped:                     "AS n")
```

### Mixed with an f-string

```python
x = 1
q = ("--sql\nSELECT 1::INTEGER "
     f"AS n{x}")
# rewritten: x = 1
# rewritten: q = ("--sql\nSELECT 1::INTEGER "
# rewritten:      f"AS n{x}")
```

## Comment before the closing parenthesis

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("""--sql
SELECT 1::INTEGER AS n"""  # c
)
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("""--sql
# fixed: SELECT 1::INTEGER AS n"""  # c
# fixed: )
# stripped: from sqlty import SQL
# stripped:
# stripped: q = SQL[tuple[str]]("""--sql
# stripped: SELECT 1::INTEGER AS n"""  # c
# stripped: )
```

## Escaped quotes

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER AS n -- \"n\")")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n -- \"n\")")
# stripped: q = "--sql\nSELECT 1::INTEGER AS n -- \"n\")"
```

## Every wrapper on a line is checked

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
a, b = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER"), SQL[tuple[int]]("--sql\nSELECT 'x'::TEXT")
# fixed: from sqlty import SQL
# fixed:
# fixed: a, b = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER"), SQL[tuple[str]]("--sql\nSELECT 'x'::TEXT")
```

## Other arguments are left alone

```python
from sqlty import SQL

q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER" + suffix)
# error:
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER" + suffix)
# rewritten: q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER" + suffix)
# stripped: from sqlty import SQL
# stripped:
# stripped: q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER" + suffix)
```
