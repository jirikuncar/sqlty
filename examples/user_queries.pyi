from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id
"""
        ],
    ) -> SQL[tuple[int, str, str]]: ...
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name
"""
        ],
    ) -> SQL[tuple[int, str]]: ...
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
        SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id ORDER BY name
    """
        ],
    ) -> SQL[tuple[int, str, str]]: ...
    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
