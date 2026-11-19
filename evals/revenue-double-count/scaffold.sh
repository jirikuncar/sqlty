#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../_project/shop.sh"
cat > app/reports.py <<'PY'
from decimal import Decimal

from sqlty import SQL

REVENUE = SQL[tuple[Decimal | None, Decimal | None]]("""--sql
SELECT SUM(o.total), SUM(i.amount)
FROM orders o JOIN order_items i ON i.order_id = o.id
WHERE o.created_at >= %s""")
PY
git add -A
git -c user.name=shop -c user.email=shop@example.com commit -q -m reports
