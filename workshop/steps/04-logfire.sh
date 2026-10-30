#!/usr/bin/env bash
# Step 4: OpenTelemetry through Logfire: requests, queries and one custom log.
source "$(dirname "$0")/lib.sh"

python3 - <<'PY'
from pathlib import Path

main = Path("main.py")
src = main.read_text()
for old, new in [
    ("import psycopg\n", "import logfire\nimport psycopg\n"),
    (
        'VERSION = os.environ.get("APP_VERSION", "dev")\n',
        'VERSION = os.environ.get("APP_VERSION", "dev")\n\n'
        'logfire.configure(service_name="duckdesk", service_version=VERSION, send_to_logfire="if-token-present")\n'
        "logfire.instrument_psycopg()\n",
    ),
    (
        'app = FastAPI(title="DuckDesk", description="Rubber ducks, rented by the hour.")\n',
        'app = FastAPI(title="DuckDesk", description="Rubber ducks, rented by the hour.")\n'
        "logfire.instrument_fastapi(app)\n",
    ),
    (
        "    with connect() as conn:\n        return execute(conn, q.BOOK.bind(booking)).fetchone()\n",
        '    logfire.info("duck {duck_id} hears about {bug!r}", **booking)\n'
        "    with connect() as conn:\n        return execute(conn, q.BOOK.bind(booking)).fetchone()\n",
    ),
]:
    assert old in src, old
    src = src.replace(old, new)
main.write_text(src)
PY

git --no-pager diff --color=always main.py | cat
uv run pytest -q
say "logfire auth && logfire projects use duckdesk  # then run the app and watch the live view"

commit "Step 4: Logfire"
