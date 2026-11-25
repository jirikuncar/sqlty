"""Rows SQLite returns against what ``first-row-of-many`` assumes.

Each query runs against tables where every non-key value repeats, so a query that can
return several rows does. A query the rule leaves alone must return at most one row.
"""

from __future__ import annotations

import sqlite3
from contextlib import closing

import pytest

from sqlty import core

SCHEMA = """
CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE, team_id INTEGER REFERENCES teams, name TEXT NOT NULL);
CREATE TABLE memberships (user_id INTEGER NOT NULL, team_id INTEGER NOT NULL, PRIMARY KEY (user_id, team_id));
"""
DATA = """
INSERT INTO teams VALUES (1, 'a'), (2, 'a');
INSERT INTO users VALUES (1, 'x@a', 1, 'n'), (2, 'y@a', 1, 'n'), (3, 'z@a', 2, 'n');
INSERT INTO memberships VALUES (1, 1), (1, 2), (2, 1);
"""

# fmt: off
QUERIES = [
    ("SELECT name FROM users WHERE id = ?", (1,)),
    ("SELECT name FROM users WHERE email = ?", ("x@a",)),
    ("SELECT name FROM users WHERE name = ?", ("n",)),
    ("SELECT name FROM users WHERE team_id = ?", (1,)),
    ("SELECT name FROM users WHERE id = ? OR id = ?", (1, 2)),
    ("SELECT name FROM users WHERE id IN (?, ?)", (1, 2)),
    ("SELECT name FROM users", ()),
    ("SELECT name FROM users LIMIT 1", ()),
    ("SELECT name FROM users ORDER BY id LIMIT 1 OFFSET 1", ()),
    ("SELECT count(*) FROM users", ()),
    ("SELECT team_id, count(*) FROM users GROUP BY team_id", ()),
    ("SELECT u.name, t.name FROM users u JOIN teams t ON t.id = u.team_id WHERE u.id = ?", (1,)),
    ("SELECT u.name, t.name FROM users u LEFT JOIN teams t ON t.id = u.team_id WHERE u.id = ?", (1,)),
    ("SELECT u.name FROM users u LEFT JOIN teams t ON u.id = ? AND t.id = u.team_id", (1,)),
    ("SELECT t.name FROM teams t JOIN users u ON u.team_id = t.id WHERE t.id = ?", (1,)),
    ("SELECT 1 FROM memberships WHERE user_id = ?", (1,)),
    ("SELECT 1 FROM memberships WHERE user_id = ? AND team_id = ?", (1, 2)),
    ("SELECT 1 FROM users, teams WHERE users.id = ? AND teams.id = users.team_id", (1,)),
    ("SELECT u.name FROM users u JOIN memberships m ON m.user_id = u.id WHERE u.id = ?", (1,)),
    ("UPDATE users SET name = ? WHERE id = ? RETURNING name", ("m", 1)),
    ("UPDATE users SET name = ? WHERE team_id = ? RETURNING name", ("m", 1)),
    ("DELETE FROM memberships WHERE user_id = ? RETURNING team_id", (1,)),
    ("INSERT INTO teams (name) VALUES (?) RETURNING id", ("b",)),
    ("INSERT INTO teams (name) VALUES (?), (?) RETURNING id", ("b", "c")),
    ("INSERT OR IGNORE INTO teams (id, name) VALUES (?, ?) RETURNING id", (1, "b")),
]
# fmt: on


def _reported(sql: str) -> bool:
    source = f'import sqlite3\nconn = sqlite3.connect("")\nconn.execute({sql!r}, ()).fetchone()\n'
    diagnostics = core.check_source(source, core.Settings(dialect="sqlite", schema_sql=SCHEMA))
    return any(d["rule"] == "first-row-of-many" for d in diagnostics)


@pytest.mark.parametrize(("sql", "params"), QUERIES)
def test_queries_not_reported_return_at_most_one_row(sql: str, params: tuple[object, ...]) -> None:
    with closing(sqlite3.connect(":memory:")) as conn:
        conn.executescript(SCHEMA + DATA)
        returned = len(conn.execute(sql, params).fetchall())
    reported = _reported(sql)
    assert reported or returned <= 1, f"returned {returned} rows, not reported"
    assert not reported or returned > 1, f"returned {returned} rows, reported"
