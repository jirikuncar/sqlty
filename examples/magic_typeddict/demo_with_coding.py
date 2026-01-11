# coding: sqlty?return-format=typeddict
"""
Example demonstrating return format specification in coding declaration.

This shows how to specify the return format directly in the coding declaration
using query parameters, overriding any pyproject.toml configuration.
"""

import datetime
from typing import TypedDict

from sqlty import SQL


# Define the expected TypedDict types as prototypes
class QueryRow_724392cb(TypedDict):
    """Prototype for query1 result type."""
    id: int
    name: str


class QueryRow_ae593e70(TypedDict):
    """Prototype for query2 result type."""
    created_at: datetime.datetime
    email: str
    id: int
    is_active: bool


# The coding declaration above specifies return-format=typeddict
# This overrides any pyproject.toml configuration in this directory

query1: SQL[QueryRow_724392cb] = "--sql SELECT id::INTEGER, name::TEXT FROM users"

query2: SQL[QueryRow_ae593e70] = """--sql
SELECT 
    id::INTEGER,
    email::TEXT,
    created_at::TIMESTAMP,
    is_active::BOOLEAN
FROM users
WHERE id = 1
"""


def main() -> None:
    """Demonstrate queries with coding declaration format override."""
    print("Magic codec with coding declaration format override")
    print(f"Format specified: typeddict (from coding declaration)")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
