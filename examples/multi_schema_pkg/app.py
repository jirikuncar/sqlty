from .orders_queries import sql as order_sql
from .users_queries import sql as user_sql

# Example usage
user_result = user_sql("SELECT id, name FROM users")
order_result = order_sql("SELECT id, total FROM orders")

# Type hints (should be SQL[tuple[int, str]] and SQL[tuple[int, float]])
