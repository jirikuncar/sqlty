"""SQL[T] is a str subclass — works anywhere str is expected.

mypy: ok
pyright: ok
ty: ok
"""

from sqlty import SQL


def process(s: str) -> str:
    return s.upper()


query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")

# SQL[T] is a str — passing to str-accepting function should work
result = process(query)
reveal_type(result)  # str
