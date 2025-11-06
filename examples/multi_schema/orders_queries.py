"""Order queries using @sqlty decorator."""

from typing import Any

from sqlty import sqlty


@sqlty(schema="orders.sql")
def sql(query: str) -> Any:
    """Execute order-related SQL queries."""
    return query


order_query = sql("SELECT id, total FROM orders")
