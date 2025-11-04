from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class UserQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            "\n        SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id ORDER BY name\n    "
        ],
    ) -> SQL[tuple[int, str, str]]: ...
    @overload
    @classmethod
    def sql(
        cls, query: Literal["\n    SELECT id::INTEGER, name::TEXT FROM users ORDER BY name\n"]
    ) -> SQL[tuple[int, str]]: ...
    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            "\n    SELECT id::INTEGER, name::TEXT, email::TEXT FROM users WHERE id = :id\n"
        ],
    ) -> SQL[tuple[int, str, str]]: ...
    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

user_sql = UserQueries.sql
