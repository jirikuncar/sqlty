from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class QueryA(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
    SELECT id, name, age FROM users WHERE id = :id
    """
        ],
    ) -> SQL[tuple[int, str, int]]:
        # from query:21
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

class QueryB(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            """
    SELECT id, name, age FROM users WHERE id = :id
    """
        ],
    ) -> SQL[tuple[int, str, float]]:
        # from query:27
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type aliases for the sql functions
sql = QueryA.sql
