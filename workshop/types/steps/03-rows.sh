#!/usr/bin/env bash
# Step 3: name the rows with dataclasses; sqlty checks them against the SQL, and explains.
source "$(dirname "$0")/lib.sh"

uv run python - <<'PY'
import re
from pathlib import Path

path = Path("queries.py")
text = path.read_text()
for name, row in [("LIST_DUCKS", "Duck"), ("BOOK", "Consultation"), ("SOLVE", "Consultation"), ("LEADERBOARD", "Champion")]:
    text = re.sub(rf"^({name} = SQL\[)tuple\[.*?\](, )", rf"\g<1>{row}\g<2>", text, flags=re.M)
path.write_text("from dataclasses import dataclass\n" + text)
PY
cat > rows.py <<'PY'


@dataclass
class Duck:
    id: int
    name: str
    colour: str
    hourly_rate: float


@dataclass
class Consultation:
    id: int
    duck_id: int
    bug: str
    solved: bool


@dataclass
class Champion:
    name: str
    bugs_solved: int
    hardest_bug: str
PY
sed -i '/^from sqlty import SQL$/r rows.py' queries.py && rm rows.py
cat >> queries.py <<'PY'

DUCK = SQL[Duck]("--sql\nSELECT id, name, colur, hourly_rate FROM ducks WHERE id = %(id)s")
PY
uv run sqlty rewrite -q .

say "three mistakes; --explain says why sqlty thinks so"
expect_failure uv run sqlty check --explain .

say "the typo has a quick fix (the editor offers the same one)"
expect_failure uv run sqlty check --fix .

say "money is never a float, and a duck with no solved bug has no hardest bug"
sed -i 's/hourly_rate: float/hourly_rate: decimal.Decimal/; s/hardest_bug: str$/hardest_bug: str | None/' queries.py
uv run sqlty check .
uv run prek run ruff-format --files queries.py >/dev/null || true
cat queries.py

commit "Step 3: rows with names"
