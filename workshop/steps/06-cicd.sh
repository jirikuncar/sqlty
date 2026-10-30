#!/usr/bin/env bash
# Step 6: GitHub Actions runs the same hooks, then deploys main to Fly.io.
source "$(dirname "$0")/lib.sh"

mkdir -p .github/workflows
cat > .github/workflows/ci.yml <<'YAML'
name: ci

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
          persist-credentials: false
      - uses: astral-sh/setup-uv@v10
      - run: uv sync --locked
      - run: uv run prek run --all-files --show-diff-on-failure

  deploy:
    needs: check
    if: github.ref == 'refs/heads/main'
    runs-on: ubuntu-latest
    concurrency: deploy
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: superfly/flyctl-actions/setup-flyctl@master
      - run: flyctl deploy --remote-only
        env:
          FLY_API_TOKEN: ${{ secrets.FLY_API_TOKEN }}
YAML

cat > Dockerfile <<'DOCKER'
FROM ghcr.io/astral-sh/uv:python3.14-trixie-slim
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy
WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --locked --no-dev --no-install-project
COPY schema.sql seed.sql queries.py main.py ./
ENV PATH=/app/.venv/bin:$PATH
CMD ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8080"]
DOCKER

printf '.venv/\n.git/\n__pycache__/\n' > .dockerignore

cat > fly.toml <<TOML
app = "${FLY_APP:-duckdesk-$(whoami)}"
primary_region = "${FLY_REGION:-ams}"

[http_service]
  internal_port = 8080
  force_https = true
  auto_stop_machines = "stop"
  auto_start_machines = true
  min_machines_running = 0
TOML

commit "Step 6: CI/CD"
git tag -f prod >/dev/null

say "once, before the talk:"
cat <<'TXT'
  gh repo create duckdesk --private --source . --push
  fly apps create "$(grep -oP '^app = "\K[^"]+' fly.toml)"
  fly secrets set LOGFIRE_TOKEN=...          # a write token of the Logfire project
  gh secret set FLY_API_TOKEN --body "$(fly tokens create deploy)"
TXT
