#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../_project/shop.sh"
python3 - <<'PY'
import pathlib, re
p = pathlib.Path("pyproject.toml")
p.write_text(re.sub(r"\n\[tool\.sqlty\]\n.*", "\n", p.read_text(), flags=re.S))
PY
cat > app/users.py <<'PY'
import psycopg


def get_user(conn: psycopg.Connection, user_id: int):
    return conn.execute("SELECT id, email, name FROM users WHERE id = %s", (user_id,)).fetchone()


def count_orders(conn: psycopg.Connection, user_id: int):
    return conn.execute("SELECT count(*) FROM orders WHERE user_id = %s", (user_id,)).fetchone()
PY
rm app/queries.py
git add -A
git -c user.name=shop -c user.email=shop@example.com commit -q -m "raw queries"
