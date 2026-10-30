#!/usr/bin/env bash
# Step 2: plain --sql strings; sqlty rewrite turns them into SQL[T].
source "$(dirname "$0")/lib.sh"

cat > queries.py <<'PY'
LIST_DUCKS = "--sql\nSELECT id, name, colour, hourly_rate FROM ducks ORDER BY hourly_rate DESC"

BOOK = "--sql\nINSERT INTO consultations (duck_id, developer_id, bug) VALUES (%(duck_id)s, %(developer_id)s, %(bug)s) RETURNING id"

SOLVE = "--sql\nUPDATE consultations SET solved = TRUE WHERE id = %(id)s"

LEADERBOARD = """--sql
SELECT d.name, count(c.id) AS bugs_solved
FROM ducks AS d
LEFT JOIN consultations AS c ON c.duck_id = d.id AND c.solved
GROUP BY d.id, d.name
ORDER BY bugs_solved DESC, d.name"""
PY

say "sqlty rewrite: the schema writes the types"
uv run sqlty rewrite .
cat queries.py

say "tuples are fine, names are nicer: swap them for dataclasses"
cat > queries.py <<'PY'
from dataclasses import dataclass
from decimal import Decimal

from sqlty import SQL


@dataclass
class Duck:
    id: int
    name: str
    colour: str
    hourly_rate: Decimal


@dataclass
class Champion:
    name: str
    bugs_solved: int


LIST_DUCKS = SQL[Duck]("--sql\nSELECT id, name, colour, hourly_rate FROM ducks ORDER BY hourly_rate DESC")

BOOK = SQL[tuple[int]](
    "--sql\nINSERT INTO consultations (duck_id, developer_id, bug) VALUES (%(duck_id)s, %(developer_id)s, %(bug)s) RETURNING id"
)

SOLVE = "--sql\nUPDATE consultations SET solved = TRUE WHERE id = %(id)s"

LEADERBOARD = SQL[Champion]("""--sql
SELECT d.name, count(c.id) AS bugs_solved
FROM ducks AS d
LEFT JOIN consultations AS c ON c.duck_id = d.id AND c.solved
GROUP BY d.id, d.name
ORDER BY bugs_solved DESC, d.name""")
PY
uv run sqlty check .

commit "Step 2: typed queries"
