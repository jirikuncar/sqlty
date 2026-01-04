# /// script
# requires-python = ">=3.14"
# dependencies = [
#     "sqlty",
# ]
# ///
"""
Example demonstrating the magic codec for --sql prefix strings.

This example shows how to use the magic codec to automatically transform
strings prefixed with --sql into SQL[T](...) calls with type inference.

To use this feature:
1. Import the magic module: from sqlty import magic
2. Use --sql prefix in your strings

The codec will automatically:
- Detect strings starting with --sql
- Analyze the SQL query
- Infer the return type
- Transform the string into SQL[T]("...")

Note: No encoding declaration change is needed. The magic codec overrides
UTF-8, so files can continue using # coding: utf-8 or no declaration.
"""

from sqlty import SQL, magic  # pyright: ignore[reportUnusedImport] # noqa: F401

# These strings will be automatically transformed by the codec

# Simple SELECT with single column
query1: SQL[tuple[int]] = "--sql SELECT 1::INTEGER"
# After transformation: query1 = SQL[tuple[int]]("--sql SELECT 1::INTEGER")

# SELECT with multiple columns
query2 = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# After transformation: query2 = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")

# Complex query
query3 = """--sql
SELECT
    id::INTEGER,
    email::TEXT,
    created_at::TIMESTAMP
FROM users
WHERE active = true
"""
# After transformation: query3 = SQL[tuple[int, str, datetime.datetime]](...)


def main() -> None:
    """Demonstrate the transformed queries."""
    print("Magic codec example")
    print(f"Query 1: {query1}", type(query1))
    print(f"Query 2: {query2}", type(query2))
    print(f"Query 3: {query3}", type(query3))


if __name__ == "__main__":
    main()
