# coding: sqlty?return-format=namedtuple
"""
Example demonstrating return format specification in coding declaration.

This shows how to specify the return format directly in the coding declaration
using query parameters, overriding any pyproject.toml configuration.

The codec generates Protocol definitions that any user-defined type with the
same structure can satisfy, enabling structural typing.
"""

from collections import namedtuple
from typing import cast

from sqlty import SQL

# The coding declaration above specifies return-format=namedtuple
# This overrides any pyproject.toml configuration in this directory

# The codec will auto-generate Protocol definitions like QueryRow_xxx
# These Protocols define the structure that return types must have

# User defines their own namedtuple types with friendly names
User = namedtuple("User", ["id", "name"])
Stats = namedtuple("Stats", ["total", "avg_age"])

# The codec transforms the query string to SQL[QueryRow_xxx](...)
# Since User satisfies QueryRow_xxx Protocol (structurally),
# and SQL is covariant, we can use cast to get the user's type
query1: SQL[User] = cast(SQL[User], "--sql SELECT id::INTEGER, name::TEXT FROM users")

query2 = cast(SQL[Stats], """--sql
SELECT
    COUNT(*)::INTEGER as total,
    AVG(age::FLOAT)::FLOAT as avg_age
FROM users
""")


def main() -> None:
    """Demonstrate queries with coding declaration format override."""
    print("Magic codec with coding declaration format override")
    print(f"Format specified: namedtuple (from coding declaration)")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
