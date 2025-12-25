"""Example demonstrating psycopg3 cursor wrapper with inferred query types.

This example shows how to wrap a psycopg3 connection to provide type-safe
query execution where the cursor is generic over a single row type.

The TypedCursor[RowType] is generic, meaning each instance is bound to a specific
row type. The stub file provides overloads that infer the RowType from literal
query strings, enabling full type inference.

Key benefits:
- Type-safe cursor wrapper bound to a single row type
- Type inference works through all cursor methods (fetchall, fetchone, fetchmany)
- Leverages psycopg3's Cursor generic type for proper typing
- Clear type constraint: one cursor instance = one row type
"""

import psycopg

from .db import TypedCursor


def main() -> None:
    """Main application demonstrating the typed cursor wrapper."""
    # Connect to PostgreSQL
    conn_str = "host=localhost port=5432 dbname=testdb user=testuser password=testpass"

    with psycopg.connect(conn_str) as conn:
        # Example 1: Simple single-column query
        # Create a cursor bound to tuple[int] row type
        cursor1: TypedCursor[tuple[int]] = TypedCursor(conn)
        cur1 = cursor1.execute("SELECT 1::INTEGER")
        result1 = cur1.fetchall()  # list[tuple[int]]
        print(f"Single integer: {result1}")

        # Example 2: Multi-column query from schema
        # Create a cursor bound to tuple[int, str] row type
        cursor2: TypedCursor[tuple[int, str]] = TypedCursor(conn)
        cur2 = cursor2.execute("SELECT id::INTEGER, name::TEXT FROM users")
        result2 = cur2.fetchall()  # list[tuple[int, str]]
        print(f"\nUsers (id, name): {result2}")

        # Example 3: Complex query with multiple columns
        # Create a cursor bound to tuple[int, str, str, int | None] row type
        cursor3: TypedCursor[tuple[int, str, str, int | None]] = TypedCursor(conn)
        cur3 = cursor3.execute(
            "SELECT id::INTEGER, name::TEXT, email::TEXT, age::INTEGER FROM users WHERE age > 25"
        )
        result3 = cur3.fetchall()  # list[tuple[int, str, str, int | None]]
        print(f"\nUsers over 25: {result3}")

        # Example 4: Single row query with fetchone
        # Reuse cursor2 since it has the same row type
        cur4 = cursor2.execute("SELECT id::INTEGER, name::TEXT FROM users WHERE id = 1")
        result4 = cur4.fetchone()  # tuple[int, str] | None
        print(f"\nSingle user: {result4}")

        # Example 5: Join query with multiple columns
        # Create a cursor bound to tuple[str, str, bool] row type
        cursor5: TypedCursor[tuple[str, str, bool]] = TypedCursor(conn)
        cur5 = cursor5.execute(
            """
            SELECT u.name::TEXT, p.title::TEXT, p.published::BOOLEAN
            FROM users u
            JOIN posts p ON u.id = p.user_id
            WHERE p.published = true
            """
        )
        result5 = cur5.fetchall()  # list[tuple[str, str, bool]]
        print(f"\nPublished posts with authors: {result5}")

        # Example 6: Aggregate query (reuse cursor1)
        cur6 = cursor1.execute("SELECT COUNT(*)::INTEGER FROM users")
        result6 = cur6.fetchall()  # list[tuple[int]]
        print(f"\nUser count: {result6}")

        # Example 7: Query with parameters
        # Create a cursor bound to tuple[str, str] row type
        cursor7: TypedCursor[tuple[str, str]] = TypedCursor(conn)
        user_id = 1
        cur7 = cursor7.execute(
            "SELECT name::TEXT, email::TEXT FROM users WHERE id = %s",
            (user_id,),
        )
        result7 = cur7.fetchall()  # list[tuple[str, str]]
        print(f"\nUser with parameterized query: {result7}")

        # Example 8: Using fetchmany (reuse cursor2)
        cur8 = cursor2.execute("SELECT id::INTEGER, name::TEXT FROM users")
        batch = cur8.fetchmany(5)  # list[tuple[int, str]]
        print(f"\nFirst 5 users: {batch}")


if __name__ == "__main__":
    main()
