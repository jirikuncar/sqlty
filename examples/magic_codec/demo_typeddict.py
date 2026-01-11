# coding: sqlty
"""
Example demonstrating TypedDict return format with the magic codec.

This example uses the magic codec with TypedDict format configured in pyproject.toml.
The codec will automatically transform --sql strings into SQL[QueryRow_xxx](...) calls
where QueryRow_xxx is a TypedDict class with proper field names and types.
"""

# Note: The actual TypedDict class would be generated in a stub file
# The magic codec transforms the string to reference the class name

# Simple query
query1 = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# Becomes: query1 = SQL[QueryRow_xxx]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# Where QueryRow_xxx is:
#   class QueryRow_xxx(TypedDict):
#       id: int
#       name: str

# Complex query with more fields
query2 = """--sql
SELECT 
    id::INTEGER,
    email::TEXT,
    created_at::TIMESTAMP,
    is_active::BOOLEAN
FROM users
WHERE id = 1
"""
# Becomes: query2 = SQL[QueryRow_yyy](...)
# Where QueryRow_yyy is:
#   class QueryRow_yyy(TypedDict):
#       created_at: datetime.datetime
#       email: str
#       id: int
#       is_active: bool


def main() -> None:
    """Demonstrate the transformed queries."""
    print("Magic codec TypedDict example")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
