from typing import Literal, overload
from sqlty import SQL, SQLRegistry

class OrdersQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT id, total FROM orders"]) -> SQL[tuple[int, float]]: ...
    @overload
    @classmethod
    def sql[T](cls, query: T) -> T: ...

sql = OrdersQueries.sql
