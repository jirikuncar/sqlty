#!/usr/bin/env bash
# Step 2: plain --sql strings; the sqlty-rewrite hook writes their types on commit.
source "$(dirname "$0")/lib.sh"

cat > queries.py <<'PY'
LIST_DUCKS = "--sql\nSELECT id, name, colour, hourly_rate FROM ducks ORDER BY hourly_rate DESC"

BOOK = "--sql\nINSERT INTO consultations (duck_id, developer_id, bug) VALUES (%(duck_id)s, %(developer_id)s, %(bug)s) RETURNING id, duck_id, bug, solved"

SOLVE = "--sql\nUPDATE consultations SET solved = TRUE WHERE id = %(id)s RETURNING id, duck_id, bug, solved"

LEADERBOARD = """--sql
SELECT d.name, count(c.id) AS bugs_solved, max(c.bug) AS hardest_bug
FROM ducks AS d
LEFT JOIN consultations AS c ON c.duck_id = d.id AND c.solved
GROUP BY d.id, d.name
ORDER BY bugs_solved DESC, d.name"""
PY

say "commit: sqlty-rewrite wraps every query in SQL[Row, Params] and stops the commit, so we see what it wrote"
git add -A
expect_failure git commit -q -m "Step 2: queries"
git diff
commit "Step 2: queries, typed by the schema"
