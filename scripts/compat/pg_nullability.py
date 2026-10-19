"""Nullability of the PostgreSQL mdtests, against rows on a live server.

Runs every query `tests/test_pg_describe.py` runs on seeded rows and writes
`reports/pg_nullability.md`: the columns sqlty types as non-null that a row has NULL
in (the test fails on those), and the columns sqlty types as nullable that no row has
NULL in. The second list is not a defect by itself: the seeded rows may not reach the
NULL, or sqlty may not prove what the query guarantees.

    SQLTY_PG_DSN=postgresql://postgres:postgres@localhost:5432/postgres \\
        uv run python scripts/compat/pg_nullability.py
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT))

import psycopg  # noqa: E402

from sqlty import core  # noqa: E402
from tests.test_pg_describe import Schemas, non_null, nullability_corpus, run  # noqa: E402


def main() -> int:
    dsn = os.environ.get("SQLTY_PG_DSN")
    if not dsn:
        print("SQLTY_PG_DSN is not set", file=sys.stderr)
        return 2
    null_in_non_null: list[str] = []
    never_null: list[str] = []
    checked = 0
    with psycopg.connect(dsn, autocommit=True) as conn:
        conn.execute("SET statement_timeout = '5s'")
        schemas = Schemas(conn)
        for section in nullability_corpus():
            schemas.load(section.schema_sql, seeded=True)
            path = section.id.rsplit(":", 1)[0]
            for line, query in section.queries:
                ours = core.analyze_select(query, dialect="postgres", schema_sql=section.schema_sql)
                rows = run(conn, query)
                if not ours or not rows or len(rows[0]) != len(ours):
                    continue
                for i, column in enumerate(ours):
                    checked += 1
                    has_null = any(row[i] is None for row in rows)
                    python_type = str(column.python_type).replace("|", "\\|")
                    entry = f"| `{path}:{line}` | `{column.name}` | `{python_type}` |"
                    if non_null(column) and has_null:
                        null_in_non_null.append(entry)
                    elif not non_null(column) and not has_null:
                        never_null.append(entry)
    report = [
        "# PostgreSQL nullability against seeded rows",
        "",
        f"{checked} columns of queries that return rows, `scripts/compat/pg_nullability.py`.",
        "",
        "## NULL in a column sqlty types as non-null",
        "",
        *_table(null_in_non_null),
        "## Nullable in sqlty, never NULL in the rows",
        "",
        *_table(never_null),
    ]
    (Path(__file__).parent / "reports" / "pg_nullability.md").write_text(
        "\n".join(report).rstrip() + "\n"
    )
    print(
        f"{checked} columns: {len(null_in_non_null)} NULL in non-null, {len(never_null)} never NULL"
    )
    return 1 if null_in_non_null else 0


def _table(entries: list[str]) -> list[str]:
    if not entries:
        return ["None.", ""]
    return ["| Query | Column | sqlty |", "|-------|--------|-------|", *entries, ""]


if __name__ == "__main__":
    raise SystemExit(main())
