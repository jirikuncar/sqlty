# coding: sqlty?return-format=pydantic
"""
Example demonstrating return format specification in coding declaration.

This shows how to specify the return format directly in the coding declaration
using query parameters, overriding any pyproject.toml configuration.

The codec generates Protocol definitions that any user-defined type with the
same structure can satisfy, enabling structural typing.
"""

import datetime
from typing import cast

from pydantic import BaseModel  # type: ignore[import-not-found]
from sqlty import SQL

# The coding declaration above specifies return-format=pydantic
# This overrides any pyproject.toml configuration in this directory

# The codec will auto-generate Protocol definitions like QueryRow_xxx
# These Protocols define the structure that return types must have

# User defines their own Pydantic model types with friendly names


class User(BaseModel):  # type: ignore[misc]
    """Pydantic model for user query."""

    id: int
    name: str
    email: str


class Post(BaseModel):  # type: ignore[misc]
    """Pydantic model for posts query."""

    id: int
    created_at: datetime.datetime
    updated_at: datetime.datetime


# The codec transforms the query string to SQL[QueryRow_xxx](...)
# Since User/Post satisfy QueryRow_xxx Protocol (structurally),
# and SQL is covariant, we can use cast to get the user's type
query1 = cast(SQL[User], "--sql SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")

query2 = cast(SQL[Post], """--sql
SELECT
    id::INTEGER,
    created_at::TIMESTAMP,
    updated_at::TIMESTAMP
FROM posts
WHERE user_id = 1
""")


def main() -> None:
    """Demonstrate queries with coding declaration format override."""
    print("Magic codec with coding declaration format override")
    print(f"Format specified: pydantic (from coding declaration)")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
