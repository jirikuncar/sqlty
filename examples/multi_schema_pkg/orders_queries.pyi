from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class OrdersQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id, total FROM orders"],
    ) -> SQL[tuple[int, float]]:
        # from app:6
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = OrdersQueries.sql
