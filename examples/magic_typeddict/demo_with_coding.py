# coding: sqlty?return-format=typeddict
"""
Example demonstrating return format specification in coding declaration.

This shows how to specify the return format directly in the coding declaration
using query parameters, overriding any pyproject.toml configuration.

The codec generates Protocol definitions that any user-defined type with the
same structure can satisfy, enabling structural typing.
"""

import datetime
from typing import TypedDict, cast

from sqlty import SQL

# The coding declaration above specifies return-format=typeddict
# This overrides any pyproject.toml configuration in this directory

# The codec will auto-generate Protocol definitions like QueryRow_xxx
# These Protocols define the structure that return types must have

# User defines their own TypedDict types with friendly names


class User(TypedDict):
    """TypedDict for basic user query."""

    id: int
    name: str


class UserDetailed(TypedDict):
    """TypedDict for detailed user query."""

    id: int
    email: str
    created_at: datetime.datetime
    is_active: bool


# The codec transforms the query string to SQL[QueryRow_xxx](...)
# Since User/UserDetailed satisfy QueryRow_xxx Protocol (structurally),
# and SQL is covariant, we can use cast to get the user's type
query1 = cast(SQL[User], "--sql SELECT id::INTEGER, name::TEXT FROM users")

query2 = cast(SQL[UserDetailed], """--sql
SELECT
    id::INTEGER,
    email::TEXT,
    created_at::TIMESTAMP,
    is_active::BOOLEAN
FROM users
WHERE id = 1
""")


def main() -> None:
    """Demonstrate queries with coding declaration format override."""
    print("Magic codec with coding declaration format override")
    print(f"Format specified: typeddict (from coding declaration)")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
