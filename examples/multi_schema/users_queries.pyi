from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class UsersQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id, name FROM users']) -> SQL[tuple[str, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = UsersQueries.sql
