# coding: sqlty
"""
Example demonstrating Pydantic return format with the magic codec.

This example uses the magic codec with Pydantic format configured in pyproject.toml.
The codec will automatically transform --sql strings into SQL[QueryRow_xxx](...) calls
where QueryRow_xxx is a Pydantic BaseModel with proper field names and types.
"""

# Note: The actual Pydantic model would be generated in a stub file
# The magic codec transforms the string to reference the class name

# Simple query
query1 = "--sql SELECT id::INTEGER, name::TEXT, email::TEXT FROM users"
# Becomes: query1 = SQL[QueryRow_xxx]("--sql SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
# Where QueryRow_xxx is:
#   class QueryRow_xxx(BaseModel):
#       id: int
#       name: str
#       email: str

# Query with datetime fields
query2 = """--sql
SELECT 
    id::INTEGER,
    created_at::TIMESTAMP,
    updated_at::TIMESTAMP
FROM posts
WHERE user_id = 1
"""
# Becomes: query2 = SQL[QueryRow_yyy](...)
# Where QueryRow_yyy is:
#   class QueryRow_yyy(BaseModel):
#       created_at: datetime.datetime
#       id: int
#       updated_at: datetime.datetime


def main() -> None:
    """Demonstrate the transformed queries."""
    print("Magic codec Pydantic example")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
