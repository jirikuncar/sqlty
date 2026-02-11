# coding: sqlty
"""
Example demonstrating return format specification in coding declaration.

The coding declaration at the top of this file can include a return-format
parameter to override the default tuple format or any pyproject.toml configuration.

Syntax:
    # coding: sqlty?return-format=<format>

Where <format> can be:
    - tuple (default): Returns tuple[type1, type2, ...]
    - namedtuple: Returns QueryRow_xxx namedtuple with field names
    - typeddict: Returns QueryRow_xxx TypedDict class
    - pydantic: Returns QueryRow_xxx Pydantic BaseModel class

This file uses the default tuple format. See the individual magic_* directories
for examples of each specific format:
    - examples/magic_namedtuple/demo_with_coding.py
    - examples/magic_typeddict/demo_with_coding.py
    - examples/magic_pydantic/demo_with_coding.py
"""

import datetime

from sqlty import SQL

# With default tuple format, queries return tuple types
query1: SQL[tuple[int, str]] = "--sql SELECT id::INTEGER, name::TEXT FROM users"

query2: SQL[tuple[int, str, datetime.datetime, bool]] = """--sql
SELECT
    id::INTEGER,
    email::TEXT,
    created_at::TIMESTAMP,
    is_active::BOOLEAN
FROM users
WHERE id = 1
"""


def main() -> None:
    """Demonstrate queries with coding declaration."""
    print("Magic codec with default tuple format")
    print(f"Query 1: {query1}")
    print(f"Query 2: {query2}")
    print()
    print("To use other formats, add a parameter to the coding declaration:")
    print("  # coding: sqlty?return-format=namedtuple")
    print("  # coding: sqlty?return-format=typeddict")
    print("  # coding: sqlty?return-format=pydantic")


if __name__ == "__main__":
    main()
