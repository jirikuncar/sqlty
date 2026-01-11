# coding: sqlty?return-format=pydantic
"""
Example demonstrating return format specification in coding declaration.

This shows how to specify the return format directly in the coding declaration
using query parameters, overriding any pyproject.toml configuration.
"""

import datetime

from pydantic import BaseModel
from sqlty import SQL


# Define the expected Pydantic model types as prototypes
class QueryRow_a7c5f825(BaseModel):
    """Prototype for query1 result type."""
    email: str
    id: int
    name: str


class QueryRow_e09be8a4(BaseModel):
    """Prototype for query2 result type."""
    created_at: datetime.datetime
    id: int
    updated_at: datetime.datetime


# The coding declaration above specifies return-format=pydantic
# This overrides any pyproject.toml configuration in this directory

query1: SQL[QueryRow_a7c5f825] = "--sql SELECT id::INTEGER, name::TEXT, email::TEXT FROM users"

query2: SQL[QueryRow_e09be8a4] = """--sql
SELECT 
    id::INTEGER,
    created_at::TIMESTAMP,
    updated_at::TIMESTAMP
FROM posts
WHERE user_id = 1
"""


def main() -> None:
    """Demonstrate queries with coding declaration format override."""
    print("Magic codec with coding declaration format override")
    print(f"Format specified: pydantic (from coding declaration)")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
