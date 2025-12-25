"""Type stubs for psycopg3 cursor wrapper with query-level type inference.

TypedCursor is generic over RowType, which represents the row type returned by queries.
The execute method uses method-level generics to enforce that the returned cursor type
matches the TypedCursor's RowType, while also inferring the type from literal queries.
"""

from typing import Any, Literal, LiteralString, overload

from psycopg import Connection, Cursor
from psycopg.abc import Params

class TypedCursor[RowType]:
    """A generic cursor wrapper that provides type inference for queries."""

    # Default constructor
    def __init__(self, conn: Connection[RowType]) -> None: ...

    # Overloaded execute methods with method-level generic R
    # The key insight: R must match RowType for the call to type-check correctly
    # When a literal query is provided, R is inferred from the overload
    # Then the type checker verifies that TypedCursor[RowType] == TypedCursor[R]

    @overload
    def execute(
        self: TypedCursor[tuple[int]],
        query: Literal["SELECT 1::INTEGER"],
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[tuple[int]]: ...

    @overload
    def execute(
        self: TypedCursor[tuple[int, str]],
        query: Literal["SELECT id::INTEGER, name::TEXT FROM users"],
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[tuple[int, str]]: ...

    @overload
    def execute(
        self: TypedCursor[tuple[int, str]],
        query: Literal["SELECT id::INTEGER, name::TEXT FROM users WHERE id = 1"],
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[tuple[int, str]]: ...

    @overload
    def execute(
        self: TypedCursor[tuple[int, str, str, int | None]],
        query: Literal[
            "SELECT id::INTEGER, name::TEXT, email::TEXT, age::INTEGER FROM users WHERE age > 25"
        ],
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[tuple[int, str, str, int | None]]: ...

    @overload
    def execute(
        self: TypedCursor[tuple[str, str, bool]],
        query: Literal[
            """
            SELECT u.name::TEXT, p.title::TEXT, p.published::BOOLEAN
            FROM users u
            JOIN posts p ON u.id = p.user_id
            WHERE p.published = true
            """
        ],
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[tuple[str, str, bool]]: ...

    @overload
    def execute(
        self: TypedCursor[tuple[int]],
        query: Literal["SELECT COUNT(*)::INTEGER FROM users"],
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[tuple[int]]: ...

    @overload
    def execute(
        self: TypedCursor[tuple[str, str]],
        query: Literal["SELECT name::TEXT, email::TEXT FROM users WHERE id = %s"],
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[tuple[str, str]]: ...

    # Fallback for non-literal queries or queries without specific overloads
    def execute(
        self,
        query: LiteralString,
        params: Params | None = None,
        *,
        prepare: bool | None = None,
        binary: bool = False,
    ) -> Cursor[RowType]: ...

    def close(self) -> None: ...
    def __enter__(self) -> TypedCursor[RowType]: ...
    def __exit__(self, *args: Any) -> None: ...

def main() -> None: ...
