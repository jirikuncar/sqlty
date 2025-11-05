from typing import Literal, overload
from sqlty import SQL, SQLRegistry

class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(
        cls, query: Literal["\n    SELECT COUNT(*)::INTEGER FROM users\n    "]
    ) -> SQL[tuple[int]]: ...
    @overload
    @classmethod
    def sql(
        cls, query: Literal["\n    SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id\n    "]
    ) -> SQL[tuple[int, str]]: ...
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            "\n    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE user_id = :user_id\n    "
        ],
    ) -> SQL[tuple[int, str, str]]: ...
    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = Query.sql
