#!/usr/bin/env bash
# Step 8: a rename is green in CI and still breaks a rolling deploy.
source "$(dirname "$0")/lib.sh"

say "the code in production against the schema of main, and the other way round"
uv run sqlty breaking --against prod || true

python3 - <<'PY'
from pathlib import Path

ci = Path(".github/workflows/ci.yml")
ci.write_text(ci.read_text().replace(
    "      - run: uv run prek run --all-files --show-diff-on-failure\n",
    "      - run: uv run prek run --all-files --show-diff-on-failure\n"
    "      - if: github.event_name == 'pull_request'\n"
    "        run: uv run sqlty breaking --against \"origin/$BASE_REF\" --output-format github\n"
    "        env:\n"
    "          BASE_REF: ${{ github.base_ref }}\n",
))
PY
git --no-pager diff --color=always | cat
commit "Step 8: check rolling deploys on pull requests"
