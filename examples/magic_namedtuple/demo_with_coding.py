# coding: sqlty?return-format=namedtuple
"""
Example demonstrating return format specification in coding declaration.

This shows how to specify the return format directly in the coding declaration
using query parameters, overriding any pyproject.toml configuration.
"""

from collections import namedtuple

from sqlty import SQL


# Define the expected namedtuple types as prototypes
QueryRow_724392cb = namedtuple('QueryRow_724392cb', ['id', 'name'])
QueryRow_bf01c4aa = namedtuple('QueryRow_bf01c4aa', ['avg_age', 'total'])


# The coding declaration above specifies return-format=namedtuple
# This overrides any pyproject.toml configuration in this directory

query1: SQL[QueryRow_724392cb] = "--sql SELECT id::INTEGER, name::TEXT FROM users"

query2: SQL[QueryRow_bf01c4aa] = """--sql
SELECT 
    COUNT(*)::INTEGER as total,
    AVG(age::FLOAT)::FLOAT as avg_age
FROM users
"""


def main() -> None:
    """Demonstrate queries with coding declaration format override."""
    print("Magic codec with coding declaration format override")
    print(f"Format specified: namedtuple (from coding declaration)")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
