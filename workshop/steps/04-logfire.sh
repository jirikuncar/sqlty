#!/usr/bin/env bash
# Step 4: OpenTelemetry through Logfire: requests, queries and one custom log.
source "$(dirname "$0")/lib.sh"

python3 - <<'PY'
from pathlib import Path

main = Path("main.py")
src = main.read_text()
src = src.replace(
    "from fastapi import FastAPI\n",
    "import logfire\nfrom fastapi import FastAPI\n",
)
src = src.replace(
    'HERE = Path(__file__).parent\n',
    'HERE = Path(__file__).parent\n\n'
    'logfire.configure(service_name="duckdesk", send_to_logfire="if-token-present")\n'
    "logfire.instrument_sqlite3()\n",
)
src = src.replace(
    'app = FastAPI(title="DuckDesk", description="Rubber ducks, rented by the hour.")\n',
    'app = FastAPI(title="DuckDesk", description="Rubber ducks, rented by the hour.")\n'
    "logfire.instrument_fastapi(app)\n",
)
src = src.replace(
    "    (consultation_id,) = db.execute(q.BOOK, booking.model_dump()).fetchone()\n",
    "    (consultation_id,) = db.execute(q.BOOK, booking.model_dump()).fetchone()\n"
    '    logfire.info("duck {duck_id} hears about {bug!r}", duck_id=booking.duck_id, bug=booking.bug)\n',
)
main.write_text(src)
PY

git --no-pager diff --color=always main.py | cat
uv run pytest -q
say "logfire auth && logfire projects use duckdesk  # then run the app and watch the live view"

commit "Step 4: Logfire"
