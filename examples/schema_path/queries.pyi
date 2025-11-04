from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class QueryA(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['\n    SELECT id, name, age FROM users WHERE id = :id\n    ']) -> SQL[tuple[int, str, int]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...

class QueryB(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['\n    SELECT id, name, age FROM users WHERE id = :id\n    ']) -> SQL[tuple[int, str, float]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
