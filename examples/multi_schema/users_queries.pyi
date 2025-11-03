from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class UsersQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id, name FROM users"],
    ) -> SQL[tuple[int, str]]:
        # from users_queries:10
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = UsersQueries.sql
