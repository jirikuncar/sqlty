#!/usr/bin/env bash
# Step 0: an empty uv project with its dependencies.
source "$(dirname "$0")/lib.sh"

rm -rf "$APP" && mkdir -p "$APP" && cd "$APP"
git init -q -b main
uv init -q --bare --app --name duckdesk --python 3.14
uv add -q fastapi uvicorn "logfire[fastapi,sqlite3]" "$SQLTY_SPEC"
uv add -q --dev pytest httpx2 prek ruff
printf '.venv/\n__pycache__/\n*.db\n' > .gitignore

commit "Step 0: an empty DuckDesk"
