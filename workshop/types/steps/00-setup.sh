#!/usr/bin/env bash
# Step 0: an empty uv project, with the published sqlty hooks and ty on every commit.
source "$(dirname "$0")/lib.sh"

if [ -f "$APP/compose.yaml" ]; then
  (cd "$APP" && docker compose down -v -t 1 2>/dev/null) || true
fi
rm -rf "$APP" && mkdir -p "$APP" && cd "$APP"
git init -q -b main
uv init -q --bare --app --name duckdesk --python 3.14
uv add -q fastapi uvicorn "psycopg[binary]" "$SQLTY_SPEC"
uv add -q --dev pytest httpx2 prek ty
printf '.venv/\n__pycache__/\n' > .gitignore
cat >> pyproject.toml <<'TOML'

[tool.ruff]
line-length = 120
TOML

cat > prek.toml <<'TOML'
default_language_version = { python = "3.14" }

[[repos]]
repo = "https://github.com/jirikuncar/sqlty"
rev = "v0.3.0"
hooks = [{ id = "sqlty-rewrite" }, { id = "sqlty-check" }]

[[repos]]
repo = "https://github.com/astral-sh/ruff-pre-commit"
rev = "v0.17.3"
hooks = [{ id = "ruff-format" }]

[[repos]]
repo = "local"
hooks = [
  { id = "ty", name = "ty", entry = "uv run ty check", language = "system", types = ["python"], pass_filenames = false },
]
TOML

say "prek installs the git hook; prepare-hooks builds the hook environments once (cached for the talk)"
uv run prek install
uv run prek prepare-hooks

commit "Step 0: an empty DuckDesk"
