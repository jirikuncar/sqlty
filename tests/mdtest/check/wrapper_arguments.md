# Wrapper Arguments

A wrapper's argument runs to its matching `)`: every implicitly concatenated
string, comments and a trailing comma included. `check`, `check --fix`,
`rewrite` and `strip` all read it as one query, so their output stays valid
Python. Where the strings can't stand alone, `strip` keeps the parentheses.

## Implicitly concatenated strings

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
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

### Parentheses kept when the strings need them

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER "
                    "AS n")
# stripped: q = ("--sql\nSELECT 1::INTEGER "
# stripped:                     "AS n")
```

## Comment before the closing parenthesis

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("""--sql
SELECT 1::INTEGER AS n"""  # c
)
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("""--sql
# fixed: SELECT 1::INTEGER AS n"""  # c
# fixed: )
# stripped: q = ("""--sql
# stripped: SELECT 1::INTEGER AS n"""  # c
# stripped: )
```

## Escaped quotes

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER AS n -- \"n\")")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n -- \"n\")")
# stripped: q = "--sql\nSELECT 1::INTEGER AS n -- \"n\")"
```

## Every wrapper on a line is checked

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
a, b = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER"), SQL[tuple[int]]("--sql\nSELECT 'x'::TEXT")
# fixed: from sqlty import SQL
# fixed:
# fixed: a, b = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER"), SQL[tuple[str]]("--sql\nSELECT 'x'::TEXT")
```

## Other arguments are reported, not edited

A wrapper whose argument is not only string literals is neither checked nor
edited; `unchecked-query` reports it.

```python
from sqlty import SQL

# error: [unchecked-query] "--sql query is not plain string literals, so it is not checked or rewritten"
q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER" + suffix)
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER" + suffix)
# rewritten: q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER" + suffix)
# stripped: from sqlty import SQL
# stripped:
# stripped: q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER" + suffix)
```

`SQL[Any]` opts out explicitly, so it is not reported. `Any` is resolved
through the module's `typing` imports, aliases and `Annotated` included.

```python
import typing
import typing_extensions as te
from typing import Annotated, Any
from typing import Any as A
from sqlty import SQL

q = SQL[Any]("--sql\nSELECT 1::INTEGER" + suffix)
r = SQL[typing.Any](r"--sql\nSELECT 1::INTEGER")
s = SQL[A]("--sql\nSELECT 1::INTEGER" + suffix)
t = SQL[te.Any]("--sql\nSELECT 1::INTEGER" + suffix)
u = SQL[Annotated[Any, "dynamic"]]("--sql\nSELECT 1::INTEGER" + suffix)
v = SQL[(Any)]("--sql\nSELECT 1::INTEGER" + suffix)
w = SQL[
    Any  # dynamic
]("--sql\nSELECT 1::INTEGER" + suffix)
ž = "ř"; x = SQL[Any]("--sql\nSELECT 1::INTEGER" + suffix)
```

## A name `Any` that is not `typing.Any` does not opt out

```python
from sqlty import SQL

class Any: ...

# error: [unchecked-query] "--sql query is not plain string literals, so it is not checked or rewritten"
q = SQL[Any]("--sql\nSELECT 1::INTEGER" + suffix)
# error: [unchecked-query] "--sql query is not plain string literals, so it is not checked or rewritten"
r = SQL[models.Any]("--sql\nSELECT 1::INTEGER" + suffix)
```

## `SQL[Any]` still reports interpolated strings

Only `unchecked-query` is skipped.

```python
from typing import Any
from sqlty import SQL

# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
q = SQL[Any](f"--sql\nSELECT {column} FROM users")
# error: [tstring-skipped] "t-string with --sql prefix skipped (the field {column:i} changes the SQL)"
r = SQL[Any](t"--sql\nSELECT {column:i} FROM users")
```

## Interpolated or raw parts skip the whole concatenation

```python
x = 1
# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
q = ("--sql\nSELECT 1::INTEGER "
     f"AS n{x}")
# error: [unchecked-query] "--sql query is not plain string literals, so it is not checked or rewritten"
r = ("--sql\nSELECT 1::INTEGER "
     r"AS n")
# rewritten: x = 1
# rewritten: q = ("--sql\nSELECT 1::INTEGER "
# rewritten:      f"AS n{x}")
# rewritten: r = ("--sql\nSELECT 1::INTEGER "
# rewritten:      r"AS n")
```

## A wrapper from another module is checked, never wrapped again

`mod.SQL[...]` and `sqlty.SQL[...]` are checked and fixed like `SQL[...]`;
`strip` leaves them alone.

```python
import mod

# error: [narrow-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = mod.SQL[tuple[str]]("--sql\nSELECT 1::INTEGER AS n")
# fixed: import mod
# fixed:
# fixed: q = mod.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
# rewritten: import mod
# rewritten:
# rewritten: q = mod.SQL[tuple[str]]("--sql\nSELECT 1::INTEGER AS n")
# stripped: import mod
# stripped:
# stripped: q = mod.SQL[tuple[str]]("--sql\nSELECT 1::INTEGER AS n")
```

## An f-string in a wrapper is reported once

```python
from sqlty import SQL

# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
q = SQL[tuple[int]](f"--sql\nSELECT {column}::INTEGER FROM users")
```

## Comments after a trailing comma are kept

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER",  # c
)
# stripped: q = ("--sql\nSELECT 1::INTEGER"  # c
# stripped: )
```

## A format spec over lines in a triple-quoted f-string

```python
from sqlty import SQL

x = 1
y = f"""{x:
}"""
z = 1
"""Example: SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")"""
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = "--sql\nSELECT 1::INTEGER AS n"
# fixed: from sqlty import SQL
# fixed:
# fixed: x = 1
# fixed: y = f"""{x:
# fixed: }"""
# fixed: z = 1
# fixed: """Example: SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")"""
# fixed: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
# stripped: from sqlty import SQL
# stripped:
# stripped: x = 1
# stripped: y = f"""{x:
# stripped: }"""
# stripped: z = 1
# stripped: """Example: SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")"""
# stripped: q = "--sql\nSELECT 1::INTEGER AS n"
```

## Nested f-string quotes don't hide a statement's end

PEP 701 lets an f-string field hold the quote and brackets of its own
string; the docstring stays a statement of its own.

```python
x = f"{"("}"
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = "--sql\nSELECT 1::INTEGER AS n"
"""Attribute doc."""
# rewritten: x = f"{"("}"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
# rewritten: """Attribute doc."""
```

### Strip keeps a multi-line wrapper valid

```python
from sqlty import SQL

x = f"{"("}"
y = 1 if"(" else 2
q = SQL[tuple[int]](
    "--sql\nSELECT 1::INTEGER "
    "AS n"
)
# stripped: x = f"{"("}"
# stripped: y = 1 if"(" else 2
# stripped: q = (
# stripped:     "--sql\nSELECT 1::INTEGER "
# stripped:     "AS n"
# stripped: )
# rewritten: x = f"{"("}"
# rewritten: y = 1 if"(" else 2
# rewritten: q = SQL[tuple[int]](
# rewritten:     "--sql\nSELECT 1::INTEGER "
# rewritten:     "AS n"
# rewritten: )
```
