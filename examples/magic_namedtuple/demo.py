# coding: sqlty
"""
Example demonstrating namedtuple return format with the magic codec.

This example uses the magic codec with namedtuple format configured in pyproject.toml.
The codec will automatically transform --sql strings into SQL[QueryRow_xxx](...) calls
where QueryRow_xxx is a namedtuple with proper field names.
"""

from collections import namedtuple

from sqlty import SQL


# Define the expected namedtuple types as prototypes
QueryRow_724392cb = namedtuple('QueryRow_724392cb', ['id', 'name'])
QueryRow_bf01c4aa = namedtuple('QueryRow_bf01c4aa', ['avg_age', 'total'])


# Simple query with explicit type annotation matching the generated type
query1: SQL[QueryRow_724392cb] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# The codec transforms this to: SQL[QueryRow_724392cb]("...")

# Query with aggregates
query2: SQL[QueryRow_bf01c4aa] = """--sql
SELECT 
    COUNT(*)::INTEGER as total,
    AVG(age::FLOAT)::FLOAT as avg_age
FROM users
"""
# The codec transforms this to: SQL[QueryRow_bf01c4aa]("...")


def main() -> None:
    """Demonstrate the transformed queries."""
    print("Magic codec namedtuple example")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
