# Strings and calls the scanner must not cut

`rewrite` and `strip` find string literals and `SQL[T](...)` calls with a scanner of their own,
so a lexing gap would shift every query after it.

## An f-string field that holds brackets and quotes

```python
a = f"{"("}"
b = f'{x:>{w}}'
c = f"{d['(']!r}"
d = t"{f'{"["}'}"
e = f"{{("
g = f"""{x  # (
}"""
q = "--sql\nSELECT 1 AS a"
# rewritten: a = f"{"("}"
# rewritten: b = f'{x:>{w}}'
# rewritten: c = f"{d['(']!r}"
# rewritten: d = t"{f'{"["}'}"
# rewritten: e = f"{{("
# rewritten: g = f"""{x  # (
# rewritten: }"""
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1 AS a")
```

## Quotes inside strings

```python
a = """a " ) '' """
b = '''x'' ' '''
c = "a\\"
d = 'a\'('
e = "a\
b"
q = "--sql\nSELECT 1 AS a"
# rewritten: a = """a " ) '' """
# rewritten: b = '''x'' ' '''
# rewritten: c = "a\\"
# rewritten: d = 'a\'('
# rewritten: e = "a\
# rewritten: b"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1 AS a")
```

## An unterminated string ends at its line

```python
x = "a(
y = 1
q = "--sql\nSELECT 1 AS a"
# rewritten: x = "a(
# rewritten: y = 1
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1 AS a")
```

## A backslash continuation joins the strings

```python
q = ("--sql\nSELECT 1 " \
    "AS a")
# rewritten: q = (SQL[tuple[int]]("--sql\nSELECT 1 " \
# rewritten:     "AS a"))
```

## A statement start resets the bracket depth

```python
x = (
def f(): pass
q = ("--sql\nSELECT 1 "
     "AS a")
# rewritten: x = (
# rewritten: def f(): pass
# rewritten: q = (SQL[tuple[int]]("--sql\nSELECT 1 "
# rewritten:      "AS a"))
```

## A matrix multiplication at a line start is not a decorator

```python
x = (a
@ b, "q "
"n")
q = "--sql\nSELECT 1 AS a"
# rewritten: x = (a
# rewritten: @ b, "q "
# rewritten: "n")
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1 AS a")
```

## Adjacent strings join only inside one expression

```python
a = ("--sql\nSELECT 1 "  # c
     "AS a")
b = "--sql\nSELECT 2 AS b"
"AS c"
x = f"{"("}"
q = "--sql\nSELECT 3 AS c"
"""doc"""
# rewritten: a = (SQL[tuple[int]]("--sql\nSELECT 1 "  # c
# rewritten:      "AS a"))
# rewritten: b = SQL[tuple[int]]("--sql\nSELECT 2 AS b")
# rewritten: "AS c"
# rewritten: x = f"{"("}"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 3 AS c")
# rewritten: """doc"""
```

## Calls with comments, concatenated strings and a dotted wrapper

```python
a = SQL[tuple[int]]("--sql\nSELECT 1 "  # c
    'AS a'  # e
)
b = SQL[T](x)
c, d = SQL[T](f("(", (1))), mod.SQL[U]("--sql\nSELECT 2 AS b")
# stripped: a = ("--sql\nSELECT 1 "  # c
# stripped:     'AS a'  # e
# stripped: )
# stripped: b = SQL[T](x)
# stripped: c, d = SQL[T](f("(", (1))), mod.SQL[U]("--sql\nSELECT 2 AS b")
```

## The row type is the first type argument

```python
a = SQL[dict[str, int], tuple[int, str]]("--sql\nSELECT 1 AS a")
b = SQL[
    tuple[int],
]('--sql\nSELECT 1 AS a')
# stripped: a = "--sql\nSELECT 1 AS a"
# stripped: b = '--sql\nSELECT 1 AS a'
```

## Which calls strip cleanly

A wrapper whose argument is only string literals, with an optional trailing comma, is unwrapped.

```python
a = SQL[T]("--sql\nSELECT 1 AS a",)
b = SQL[T]("--sql\nSELECT 1 " "AS a")
c = SQL[T]("--sql\nSELECT 1 " \
    "AS a")
d = SQL[T]("--sql\nSELECT 1 "
    "AS a")
e = (SQL[T]("--sql\nSELECT 1 "
    "AS a"))
f = (SQL[T](  # c
"--sql\nSELECT 1 AS a"))
g = SQL[T](f"--sql\nSELECT 1 AS a")
h = SQL[T]("--sql\nSELECT 1 AS a",  # c
)
# stripped: a = "--sql\nSELECT 1 AS a"
# stripped: b = "--sql\nSELECT 1 " "AS a"
# stripped: c = "--sql\nSELECT 1 " \
# stripped:     "AS a"
# stripped: d = ("--sql\nSELECT 1 "
# stripped:     "AS a")
# stripped: e = ("--sql\nSELECT 1 "
# stripped:     "AS a")
# stripped: f = ((  # c
# stripped: "--sql\nSELECT 1 AS a"))
# stripped: g = SQL[T](f"--sql\nSELECT 1 AS a")
# stripped: h = ("--sql\nSELECT 1 AS a"  # c
# stripped: )
```

## A call nested in another

```python
a = SQL[B](SQL[C]("--sql\nSELECT 1 AS a"))
b = SQL[A]("--sql\nSELECT 2 AS b")
# stripped: a = SQL[B]("--sql\nSELECT 1 AS a")
# stripped: b = "--sql\nSELECT 2 AS b"
```

## An unclosed call is not one

```python
a = SQL[A("a")
b = SQL[A]()
c = "--sql\nSELECT 1 AS a"
# stripped: a = SQL[A("a")
# stripped: b = SQL[A]()
# stripped: c = "--sql\nSELECT 1 AS a"
```

## A string inside a wrapper call is not wrapped again

```python
a = SQL[B](SQL[C]("--sql\nSELECT 1 AS a"))
b = SQL[B]("--sql\nSELECT 2 AS b", "--sql\nSELECT 3 AS c")
c = "--sql\nSELECT 4 AS d"
# rewritten: a = SQL[B](SQL[C]("--sql\nSELECT 1 AS a"))
# rewritten: b = SQL[B]("--sql\nSELECT 2 AS b", "--sql\nSELECT 3 AS c")
# rewritten: c = SQL[tuple[int]]("--sql\nSELECT 4 AS d")
```

## Adjacent strings inside brackets

```python
a = {"--sql\nSELECT 1 "
     "AS a"}
b = f(g(), "--sql\nSELECT 2 "
     "AS b")
c = ("--sql\nSELECT 3 "
     "AS c"
# rewritten: a = {SQL[tuple[int]]("--sql\nSELECT 1 "
# rewritten:      "AS a")}
# rewritten: b = f(g(), SQL[tuple[int]]("--sql\nSELECT 2 "
# rewritten:      "AS b"))
# rewritten: c = (SQL[tuple[int]]("--sql\nSELECT 3 ")
# rewritten:      "AS c"
```

## An unterminated f-string field

```python
a = f"{a"
b = f"{a:x"
q = "--sql\nSELECT 1 AS a"
# rewritten: a = f"{a"
# rewritten: b = f"{a:x"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1 AS a")
```
