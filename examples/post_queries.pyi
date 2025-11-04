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
        SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id ORDER BY created_at DESC
    """
        ],
    ) -> SQL[tuple[int, str]]: ...
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE id = :id
"""
        ],
    ) -> SQL[tuple[int, str, str]]: ...
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
    SELECT id::INTEGER, title::TEXT FROM posts ORDER BY created_at DESC
"""
        ],
    ) -> SQL[tuple[int, str]]: ...
    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
