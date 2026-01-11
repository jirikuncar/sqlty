# coding: sqlty
"""
Example demonstrating Pydantic return format with the magic codec.

This example uses the magic codec with Pydantic format configured in pyproject.toml.
The codec will automatically transform --sql strings into SQL[QueryRow_xxx](...) calls
where QueryRow_xxx is a Pydantic BaseModel with proper field names and types.
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


# Simple query with explicit type annotation matching the generated type
query1: SQL[QueryRow_a7c5f825] = "--sql SELECT id::INTEGER, name::TEXT, email::TEXT FROM users"
# The codec transforms this to: SQL[QueryRow_a7c5f825]("...")

# Query with datetime fields
query2: SQL[QueryRow_e09be8a4] = """--sql
SELECT 
    id::INTEGER,
    created_at::TIMESTAMP,
    updated_at::TIMESTAMP
FROM posts
WHERE user_id = 1
"""
# The codec transforms this to: SQL[QueryRow_e09be8a4]("...")


def main() -> None:
    """Demonstrate the transformed queries."""
    print("Magic codec Pydantic example")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
