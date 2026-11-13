"""Property-based tests of query parameters: placeholders, call diagnostics and ``NoParams``.

sqlite3 is the oracle: it binds a placeholder from a mapping by name and ignores extra keys.
``SQLTY_FUZZ_EXAMPLES`` raises the number of examples (default 100)::

    SQLTY_FUZZ_EXAMPLES=5000 uv run pytest tests/test_params_fuzz.py -n 0 -p no:randomly
"""

from __future__ import annotations

import os
import re
import sqlite3

from hypothesis import given, settings
from hypothesis import strategies as st

from sqlty import core

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "100"))
SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);"
NAMES = st.sampled_from(["id", "name", "age", "a", "b_2", "_x"])
SIGILS = st.sampled_from([":", "@", "$"])
# Text that looks like a placeholder but is not one: in a literal, a comment, a cast or a name.
NOISE = st.sampled_from(
    [
        "'@a :b $c ?'",
        '"name"',
        "1",
        "name || ':x'",
        "CAST(age AS INTEGER)",
    ]
)
fuzz = settings(max_examples=EXAMPLES, deadline=None)


@st.composite
def named_query(draw: st.DrawFn) -> tuple[str, set[str]]:
    """A SQLite query with named placeholders in any sigil, and the names it binds."""
    names = draw(st.lists(NAMES, max_size=4))
    terms = [f"id = {draw(SIGILS)}{name}" for name in names]
    terms += [f"{draw(NOISE)} IS NOT NULL" for _ in range(draw(st.integers(0, 2)))]
    terms = draw(st.permutations(terms)) or ["1 = 1"]
    comment = draw(st.sampled_from(["", " -- :c @d", " /* $e */"]))
    return f"SELECT id FROM users WHERE {' OR '.join(terms)}{comment}", set(names)


def bound_names(sql: str, params: dict[str, object]) -> bool:
    """Whether sqlite3 runs ``sql`` with ``params``."""
    conn = sqlite3.connect(":memory:")
    conn.execute(SCHEMA)
    try:
        conn.execute(sql, params)
    except sqlite3.ProgrammingError:
        return False
    return True


@fuzz
@given(named_query())
def test_sqlite_names_match_the_driver(case: tuple[str, set[str]]) -> None:
    sql, names = case
    params = core.analyze_params(sql, dialect="sqlite", schema_sql=SCHEMA)
    assert params is not None
    found = {p.name for p in params}
    assert found == names
    assert bound_names(sql, dict.fromkeys(found, 1))
    for name in found:
        assert not bound_names(sql, dict.fromkeys(found - {name}, 1))


@fuzz
@given(named_query(), st.sets(NAMES, max_size=5))
def test_call_diagnostics_match_the_driver(case: tuple[str, set[str]], keys: set[str]) -> None:
    sql, names = case
    source = f"import sqlite3\n\nconn = sqlite3.connect('db')\nconn.execute({sql!r}, {dict.fromkeys(sorted(keys), 1)!r})\n"
    found = {
        d["rule"]: d["message"]
        for d in core.check_source(source, dialect="sqlite", schema_sql=SCHEMA)
        if d["rule"] in ("param-count-mismatch", "extra-param")
    }
    missing, extra = sorted(names - keys), sorted(keys - names)
    assert (
        ("param-count-mismatch" in found)
        == bool(missing)
        == (not bound_names(sql, dict.fromkeys(keys, 1)))
    )
    assert ("extra-param" in found) == bool(extra)
    for rule, wanted in (("param-count-mismatch", missing), ("extra-param", extra)):
        if wanted:
            assert set(re.findall(r"'(\w+)'", found[rule])) == set(wanted)


LINES = [
    "import os",
    "from sqlty import Bound",
    "from sqlty.drivers.psycopg import execute",
    "# NoParams",
]
HEADERS = st.lists(
    st.sampled_from(['"""Doc."""', "from __future__ import annotations", *LINES]),
    unique=True,
    max_size=4,
)
FOOTERS = st.lists(st.sampled_from(LINES), unique=True, max_size=3)
QUERIES = st.sampled_from(
    [
        "SELECT name FROM users",
        "SELECT id FROM users WHERE id = %s",
        "SELECT id FROM users WHERE id = %(id)s",
        "SELECT count(*) FROM users",
    ]
)


@fuzz
@given(
    HEADERS,
    st.lists(QUERIES, min_size=1, max_size=4),
    FOOTERS,
    st.sampled_from([None, "import sqlty", "from sqlty import SQL as Q"]),
)
def test_param_types_rewrite_runs_and_strips(
    above: list[str], queries: list[str], below: list[str], style: str | None
) -> None:
    """``rewrite`` with ``param-types`` writes code that runs, and ``strip`` undoes it."""
    above = sorted(above, key=lambda line: (not line.startswith('"""'), "__future__" not in line))
    assignments = [f"q{i} = {('--sql' + chr(10) + sql)!r}" for i, sql in enumerate(queries)]
    source = "\n".join([*above, *assignments, *below]) + "\n"
    options = {"schema_sql": SCHEMA, "import_style": style}
    rewritten = core.rewrite_source(source, param_types=True, **options)
    assert core.rewrite_source(rewritten, param_types=True, **options) == rewritten
    exec(compile(rewritten, "<rewritten>", "exec"), {})
    diagnostics = core.check_source(rewritten, **options)
    assert not [d for d in diagnostics if d["rule"] == "params-type-mismatch"], rewritten
    if "# NoParams" not in source:
        assert core.strip_source(rewritten, import_style=style) == source
