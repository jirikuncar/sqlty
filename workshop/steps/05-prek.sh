#!/usr/bin/env bash
# Step 5: prek runs ruff, sqlty and pytest before every commit and every merge.
source "$(dirname "$0")/lib.sh"

cat > prek.toml <<'TOML'
default_install_hook_types = ["pre-commit", "pre-merge-commit"]
default_stages = ["pre-commit", "pre-merge-commit"]

[[repos]]
repo = "https://github.com/astral-sh/ruff-pre-commit"
rev = "v0.16.10"
hooks = [{ id = "ruff-check", args = ["--fix"] }, { id = "ruff-format" }]

[[repos]]
repo = "local"

[[repos.hooks]]
id = "sqlty-rewrite"
name = "sqlty rewrite"
entry = "uv run sqlty rewrite ."
language = "system"
pass_filenames = false
files = '\.(py|sql)$'

[[repos.hooks]]
id = "sqlty-check"
name = "sqlty check"
entry = "uv run sqlty check ."
language = "system"
pass_filenames = false
files = '\.(py|sql|toml)$'

[[repos.hooks]]
id = "pytest"
name = "pytest"
entry = "uv run pytest -q --tb=line"
language = "system"
pass_filenames = false
files = '\.(py|sql)$'
TOML

cat >> pyproject.toml <<'TOML'

[tool.ruff]
line-length = 120
TOML

mkdir -p .vscode
cat > .vscode/extensions.json <<'JSON'
{ "recommendations": ["jirikuncar.sqlty", "charliermarsh.ruff"] }
JSON
cat > .vscode/settings.json <<'JSON'
{
  "[python]": {
    "editor.codeActionsOnSave": { "source.fixAll.sqlty": "explicit" }
  }
}
JSON

uv run prek install
commit "Step 5: prek and the editor"

say "a new endpoint, written in a hurry"
cat >> queries.py <<'PY'


@dataclass
class Backlog:
    open_bugs: str


BACKLOG = SQL[Backlog]("--sql\nSELECT count(*) AS open_bugs FROM consultations WHERE NOT solved")
PY
cat >> main.py <<'PY'


@app.get("/backlog")
def backlog() -> q.Backlog | None:
    with connect() as conn:
        return execute(conn, q.BACKLOG).fetchone()
PY

git add -A
expect_failure git commit -q -m "Step 5: the backlog"

say "prek said no: count(*) is an int. The duck agrees."
sed -i 's/open_bugs: str/open_bugs: int/' queries.py
commit "Step 5: the backlog, typed"
