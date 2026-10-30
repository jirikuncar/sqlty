#!/usr/bin/env bash
# Step 8: the rename passes every check and still breaks the version in production.
# sqlty breaking stops the deploy; expand and contract ship it in two.
source "$(dirname "$0")/lib.sh"

say "deploy.sh: refuse what the version in production can't survive"
python3 - <<'PY'
from pathlib import Path

deploy = Path("deploy.sh")
build = 'git archive "$ref" | docker build'
src = deploy.read_text()
assert build in src
deploy.write_text(src.replace(build, (
    "if git rev-parse -q --verify prod >/dev/null; then\n"
    '  uv run sqlty breaking --against prod --head "$ref" --direction backward\n'
    "fi\n"
) + build, 1))
PY
git --no-pager diff --color=always deploy.sh | cat
commit "Check the running version against the new schema before deploying"

say "git push: prod still reads colour"
expect_failure git push -q

say "expand: add color next to colour, so both versions can run"
cat > migrations/003_american_spelling.sql <<'SQL'
ALTER TABLE ducks ADD COLUMN color TEXT NOT NULL DEFAULT '';
UPDATE ducks SET color = colour;
SQL
commit "Expand: color next to colour"

./traffic.sh & traffic=$!
git push -q
kill "$traffic"

say "contract: nothing in production reads colour any more"
cat > migrations/004_drop_colour.sql <<'SQL'
ALTER TABLE ducks DROP COLUMN colour;
SQL
commit "Contract: drop colour"
git push -q
