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
    SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id
    """
        ],
    ) -> SQL[tuple[int, str]]:
        # from multi_query:19
        ...

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE user_id = :user_id
    """
        ],
    ) -> SQL[tuple[int, str, str]]:
        # from multi_query:25
        ...

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
    SELECT COUNT(*)::INTEGER FROM users
    """
        ],
    ) -> SQL[tuple[int]]:
        # from multi_query:31
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = Query.sql
