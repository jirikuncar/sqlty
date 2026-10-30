#!/usr/bin/env bash
# Step 0: an empty uv project with its dependencies.
source "$(dirname "$0")/lib.sh"

if [ -f "$APP/compose.yaml" ]; then
  docker ps -aq --filter label=app=duckdesk | xargs -r docker rm -f >/dev/null
  (cd "$APP" && docker compose down -v -t 1 2>/dev/null) || true
fi
rm -rf "$APP" "$APP-origin.git" && mkdir -p "$APP" && cd "$APP"
git init -q -b main
uv init -q --bare --app --name duckdesk --python 3.14
uv add -q fastapi uvicorn "psycopg[binary]" "logfire[fastapi,psycopg]" "$SQLTY_SPEC"
uv add -q --dev pytest httpx2 prek ruff ty
printf '.venv/\n__pycache__/\n' > .gitignore
cat >> pyproject.toml <<'TOML'

[tool.ruff]
line-length = 120
TOML

commit "Step 0: an empty DuckDesk"
