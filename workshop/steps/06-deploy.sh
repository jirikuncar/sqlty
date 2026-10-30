#!/usr/bin/env bash
# Step 6: git push is the deploy button. prek builds the image, migrates and rolls it out.
source "$(dirname "$0")/lib.sh"

cat > Dockerfile <<'DOCKER'
FROM python:3.14-slim
RUN pip install --no-cache-dir uv
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_PROJECT_ENVIRONMENT=/venv PATH=/venv/bin:$PATH
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/root/.cache/uv uv sync --locked --no-dev
CMD ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8000"]
DOCKER

cat > deploy.sh <<'BASH'
#!/usr/bin/env bash
# Rolling deploy of a commit: migrate, start it next to the running version, then retire that one.
set -euo pipefail

if [ -n "${PRE_COMMIT_REMOTE_BRANCH:-}" ] && [ "$PRE_COMMIT_REMOTE_BRANCH" != refs/heads/main ]; then
  exit 0
fi
ref="${1:-${PRE_COMMIT_TO_REF:-HEAD}}"
version="$(git rev-parse --short "$ref")"
db="postgresql://duck:quack@db:5432/duckdesk"

git archive "$ref" | docker build -q -t "duckdesk:$version" - >/dev/null
docker compose up -d --wait db >/dev/null 2>&1
docker run --rm --network duckdesk -e DATABASE_URL="$db" "duckdesk:$version" python migrate.py

docker rm -f "duckdesk-$version" >/dev/null 2>&1 || true
old="$(docker ps -q --filter label=app=duckdesk)"
docker run -d --name "duckdesk-$version" --label app=duckdesk --network duckdesk -p 127.0.0.1::8000 \
  -e DATABASE_URL="$db" -e APP_VERSION="$version" -e LOGFIRE_TOKEN "duckdesk:$version" >/dev/null
port="$(docker port "duckdesk-$version" 8000 | head -1 | cut -d: -f2)"
for _ in $(seq 60); do curl -fs "localhost:$port/" >/dev/null && break; sleep 0.5; done
curl -fs "localhost:$port/" >/dev/null
echo "duckdesk $version is live on http://localhost:$port"

if [ -n "$old" ]; then
  sleep "${SOAK:-15}"
  docker rm -f $old >/dev/null
fi
git tag -f prod "$ref" >/dev/null
BASH

cat > traffic.sh <<'BASH'
#!/usr/bin/env bash
# Every running version, asked for its ducks twice a second.
while true; do
  for name in $(docker ps --filter label=app=duckdesk --format '{{.Names}}'); do
    port="$(docker port "$name" 8000 2>/dev/null | head -1 | cut -d: -f2)"
    [ -n "$port" ] || continue
    response="$(curl -s -w '\n%{http_code}' "localhost:$port/ducks")"
    printf '%s %s %s %s\n' "$(date +%T)" "$name" "${response##*$'\n'}" "$(cut -c1-60 <<<"${response%$'\n'*}")"
  done
  sleep 0.5
done
BASH
chmod +x deploy.sh traffic.sh

python3 - <<'PY'
from pathlib import Path

prek = Path("prek.toml")
prek.write_text(
    prek.read_text().replace(
        'default_install_hook_types = ["pre-commit", "pre-merge-commit"]',
        'default_install_hook_types = ["pre-commit", "pre-merge-commit", "pre-push"]',
    )
    + """
[[repos.hooks]]
id = "deploy"
name = "deploy"
entry = "./deploy.sh"
language = "system"
pass_filenames = false
always_run = true
stages = ["pre-push"]
"""
)
PY

uv run prek install
commit "Step 6: push to deploy"

git init -q --bare "$APP-origin.git"
git remote add origin "$APP-origin.git"
say "git push: the deploy button"
git push -q -u origin main
curl -s "localhost:$(docker port "duckdesk-$(git rev-parse --short prod)" 8000 | head -1 | cut -d: -f2)/"
echo
