# coding: sqlty
"""
Example demonstrating TypedDict return format with the magic codec.

This example uses the magic codec with TypedDict format configured in pyproject.toml.
The codec will automatically transform --sql strings into SQL[QueryRow_xxx](...) calls
where QueryRow_xxx is a TypedDict class with proper field names and types.
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


# Simple query with explicit type annotation matching the generated type
query1: SQL[QueryRow_724392cb] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# The codec transforms this to: SQL[QueryRow_724392cb]("...")

# Complex query with datetime fields
query2: SQL[QueryRow_ae593e70] = """--sql
SELECT 
    id::INTEGER,
    email::TEXT,
    created_at::TIMESTAMP,
    is_active::BOOLEAN
FROM users
WHERE id = 1
"""
# The codec transforms this to: SQL[QueryRow_ae593e70]("...")


def main() -> None:
    """Demonstrate the transformed queries."""
    print("Magic codec TypedDict example")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
