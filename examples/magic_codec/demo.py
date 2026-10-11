# coding: sqlty
"""
Example demonstrating the magic codec for --sql prefix strings.

This example shows how to use the magic codec to automatically transform
strings prefixed with --sql into SQL[T](...) calls with type inference.

Run it with sqlty installed from a wheel, which ships sqlty_magic.pth to
register the codec at startup: python examples/magic_codec/demo.py

The codec will automatically:
- Detect strings starting with --sql
- Analyze the SQL query
- Infer the return type
- Transform the string into SQL[T]("...")

Only modules declaring ``# coding: sqlty`` are transformed.
"""

import datetime

from sqlty import SQL

# These strings will be automatically transformed by the codec

# Simple SELECT with single column
query1: SQL[tuple[int]] = """--sql
SELECT 1::INTEGER
"""
# After transformation: query1 = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER\n")

# SELECT with multiple columns
query2 = """--sql
SELECT id::INTEGER, name::TEXT
FROM users
"""
# After transformation: query2 = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT\nFROM users\n")

# Complex query
query3: SQL[tuple[int, str, datetime.datetime]] = """--sql
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
    print(f"Query 1: {query1}", type(query1), repr(query1))
    print(f"Query 2: {query2}", type(query2), repr(query2))
    print(f"Query 3: {query3}", type(query3), repr(query3))


if __name__ == "__main__":
    main()
