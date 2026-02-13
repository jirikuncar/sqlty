"""SQL[T] preserves type parameter for static analysis.

mypy: revealed: SQL[tuple[int, str]]
pyright: revealed: SQL[tuple[int, str]]
ty: revealed: SQL[tuple[int, str]]
"""

from sqlty import SQL

# Tuple type annotation
query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
reveal_type(query)  # SQL[tuple[int, str]]
