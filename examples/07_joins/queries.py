import decimal
from sqlty import SQL
from sqlty import NoParams

# Each order repeats once per item, so AVG(o.total) weighs it by its item count.
segment_totals = SQL[tuple[str, decimal.Decimal, decimal.Decimal], NoParams]("""--sql
SELECT c.segment, AVG(o.total) AS avg_order, SUM(i.amount) AS revenue
FROM orders o
JOIN customers c ON c.id = o.customer_id
JOIN order_items i ON i.order_id = o.id
GROUP BY c.segment""")

# Aggregated per order first: one row per order, no warning.
segment_totals_fixed = SQL[tuple[str, decimal.Decimal, decimal.Decimal], NoParams]("""--sql
WITH items AS (
    SELECT order_id, SUM(amount) AS amount FROM order_items GROUP BY order_id
)
SELECT c.segment, AVG(o.total) AS avg_order, SUM(i.amount) AS revenue
FROM orders o
JOIN customers c ON c.id = o.customer_id
JOIN items i ON i.order_id = o.id
GROUP BY c.segment""")

# Items and payments both join orders one-to-many: each repeats the other.
order_balance = SQL[tuple[int, decimal.Decimal, decimal.Decimal], NoParams]("""--sql
SELECT o.id, SUM(i.amount) AS billed, SUM(p.amount) AS paid
FROM orders o
JOIN order_items i ON i.order_id = o.id
JOIN payments p ON p.order_id = o.id
GROUP BY o.id""")

# products.id holds products keys, orders.customer_id customers keys.
wrong_key = SQL[tuple[str, int], NoParams]("""--sql
SELECT p.name, o.id
FROM orders o
JOIN products p ON p.id = o.customer_id""")
