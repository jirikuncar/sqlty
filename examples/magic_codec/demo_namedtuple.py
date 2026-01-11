# coding: sqlty
"""
Example demonstrating namedtuple return format with the magic codec.

This example uses the magic codec with namedtuple format configured in pyproject.toml.
The codec will automatically transform --sql strings into SQL[QueryRow_xxx](...) calls
where QueryRow_xxx is a namedtuple with proper field names.
"""

# Note: The actual namedtuple would be generated in a stub file
# The magic codec transforms the string to reference the class name

# Simple query
query1 = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# Becomes: query1 = SQL[QueryRow_xxx]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# Where QueryRow_xxx is:
#   QueryRow_xxx = namedtuple('QueryRow_xxx', ['id', 'name'])

# Query with aggregates
query2 = "--sql SELECT COUNT(*)::INTEGER as total, AVG(age::FLOAT)::FLOAT as avg_age FROM users"
# Becomes: query2 = SQL[QueryRow_yyy](...)
# Where QueryRow_yyy is:
#   QueryRow_yyy = namedtuple('QueryRow_yyy', ['avg_age', 'total'])


def main() -> None:
    """Demonstrate the transformed queries."""
    print("Magic codec namedtuple example")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")


if __name__ == "__main__":
    main()
