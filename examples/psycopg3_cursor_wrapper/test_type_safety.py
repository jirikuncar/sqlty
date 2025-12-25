"""Test suite demonstrating type safety and mismatch detection.

Run this file with: uv run pyright test_type_safety.py

Expected behavior:
- test_correct_types() should pass type checking
- test_type_mismatches() should fail type checking with specific errors
"""

import psycopg

from .db import TypedCursor


def test_correct_types() -> None:
    """These should all pass type checking."""
    conn_str = "host=localhost"

    with psycopg.connect(conn_str) as conn:
        # ✅ Correct: cursor type matches query return type
        cursor1: TypedCursor[tuple[int]] = TypedCursor(conn)
        cur1 = cursor1.execute("SELECT 1::INTEGER")
        result1: list[tuple[int]] = cur1.fetchall()

        # ✅ Correct: cursor type matches query return type
        cursor2: TypedCursor[tuple[int, str]] = TypedCursor(conn)
        cur2 = cursor2.execute("SELECT id::INTEGER, name::TEXT FROM users")
        result2: list[tuple[int, str]] = cur2.fetchall()

        # ✅ Correct: reusing cursor with same row type
        cur3 = cursor2.execute("SELECT id::INTEGER, name::TEXT FROM users WHERE id = 1")
        result3: tuple[int, str] | None = cur3.fetchone()

        print("All correct types passed!")


def test_type_mismatches() -> None:
    """These should all FAIL type checking - uncomment to see errors."""
    conn_str = "host=localhost"

    with psycopg.connect(conn_str) as conn:
        # ❌ ERROR: cursor expects tuple[str] but query returns tuple[int]
        # Uncomment the next 2 lines to see the error:
        # cursor_wrong: TypedCursor[tuple[str]] = TypedCursor(conn)
        # cur_wrong = cursor_wrong.execute("SELECT 1::INTEGER")

        # ❌ ERROR: cursor expects tuple[int] but query returns tuple[int, str]
        # Uncomment the next 2 lines to see the error:
        # cursor_int: TypedCursor[tuple[int]] = TypedCursor(conn)
        # cur_int = cursor_int.execute("SELECT id::INTEGER, name::TEXT FROM users")

        # ❌ ERROR: cursor expects tuple[str, str] but query returns tuple[int, str]
        # Uncomment the next 2 lines to see the error:
        # cursor_ss: TypedCursor[tuple[str, str]] = TypedCursor(conn)
        # cur_ss = cursor_ss.execute("SELECT id::INTEGER, name::TEXT FROM users")

        print("If you see this, uncomment the error cases above!")


if __name__ == "__main__":
    print("Testing correct types...")
    test_correct_types()

    print("\nTesting type mismatches (errors are expected when uncommenting)...")
    test_type_mismatches()

    print("\n✅ Type safety demonstration complete!")
    print("To see type errors, uncomment the lines in test_type_mismatches()")
