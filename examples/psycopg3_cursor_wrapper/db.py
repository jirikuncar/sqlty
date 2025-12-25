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

from typing import Any, LiteralString

from psycopg import Connection, Cursor
from psycopg.abc import Params


class TypedCursor[RowType]:
    """A generic cursor wrapper that provides type inference for queries.

    The cursor is generic over RowType, which represents the row type returned
    by queries. Each instance of TypedCursor is bound to a single row type.
    """

    def __init__(self, conn: Connection[RowType]) -> None:
        """Initialize the typed cursor with a connection.

        Args:
            conn: A psycopg connection configured to return tuple rows.
        """
        self._conn = conn

    def execute(
        self,
        query: LiteralString,
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[RowType]:
        """Execute a query and return a cursor with typed rows.

        The RowType is inferred from the query string via stub file overloads.

        Args:
            query: SQL query string (literal strings get type inference via stubs).
            params: Optional query parameters.
            prepare: Whether to prepare the query. None means automatic.
            binary: Whether to return binary data.

        Returns:
            A cursor object that can be used to fetch results. The RowType
            is inferred from the SQL query via the .pyi stub file.

        Example:
            >>> cursor: TypedCursor[tuple[int, str]] = TypedCursor(conn)
            >>> cur = cursor.execute("SELECT id::INTEGER, name::TEXT FROM users")
            >>> # Type: Cursor[tuple[int, str]]
            >>> result = cur.fetchall()  # list[tuple[int, str]]
        """
        return self._conn.execute(query, params, prepare=prepare, binary=binary)  # type: ignore[return-value]

    def close(self) -> None:
        """Close the connection (cursors are auto-closed in context managers)."""
        pass

    def __enter__(self) -> "TypedCursor[RowType]":
        """Context manager entry."""
        return self

    def __exit__(self, *args: Any) -> None:
        """Context manager exit."""
        pass
