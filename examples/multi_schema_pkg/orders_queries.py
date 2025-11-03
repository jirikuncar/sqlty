from sqlty import SQLRegistry


class OrdersQueries(SQLRegistry):
    __schema_path__ = "orders.sql"


sql = OrdersQueries.sql

order_query = sql("SELECT id, total FROM orders")
