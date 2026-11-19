---
type: llm
focus:
  source: file
  path: app/reports.py
---

`REVENUE` in `app/reports.py` returns the total of `orders.total` counted once per order, and the total of `order_items.amount`, for orders created since the parameter. Joining `orders` to `order_items` before summing `orders.total` repeats each order once per item and fails; so does `DISTINCT` inside the `SUM`, which collapses two orders with the same total.
