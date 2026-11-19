#!/usr/bin/env bash
# Shared starting project: a small psycopg app with a schema, typed queries and sqlty in .venv.
# The runner gives scaffolds an empty HOME and no network, so .venv links to the one run.sh made.
set -euo pipefail
venv="$(cat "$(dirname "${BASH_SOURCE[0]}")/venv" 2>/dev/null)" || { echo "no venv: run evals/run.sh" >&2; exit 1; }
mkdir -p app db/migrations
cat > db/migrations/0001_init.sql <<'SQL'
CREATE TABLE users (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  email text NOT NULL,
  name text,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE orders (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id bigint NOT NULL REFERENCES users (id),
  total numeric(12, 2) NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE order_items (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  order_id bigint NOT NULL REFERENCES orders (id),
  sku text NOT NULL,
  amount numeric(12, 2) NOT NULL
);
SQL
cat > pyproject.toml <<'TOML'
[project]
name = "shop"
version = "0.1.0"
requires-python = ">=3.14"
dependencies = ["psycopg>=3.2", "sqlty"]

[tool.sqlty]
dialect = "postgres"
schema = "db/migrations"
TOML
cat > app/__init__.py <<'PY'
PY
cat > app/queries.py <<'PY'
import datetime

from sqlty import SQL

USER = SQL[tuple[int, str, str | None]]("--sql\nSELECT id, email, name FROM users WHERE id = %s")

RECENT_ORDERS = SQL[tuple[int, datetime.datetime]]("""--sql
SELECT id, created_at FROM orders
WHERE user_id = %s
ORDER BY created_at DESC
LIMIT 10""")
PY
cat > README.md <<'MD'
# shop

Python 3.14 and Postgres 16, psycopg 3. Queries live in `app/`; migrations are
plain SQL files in `db/migrations`, applied in order. The dev tools are installed
in `.venv` (`source .venv/bin/activate`).

We deploy with rolling updates: for a few minutes the old and the new version
of the app run side by side against the same database, and migrations run
before the new version starts.
MD
ln -s "$venv" .venv
git init -q -b main
git add -A
git -c user.name=shop -c user.email=shop@example.com commit -q -m init
