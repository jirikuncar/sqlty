"""Nullability of the MySQL mdtests, against the nullability MySQL derives.

Runs every query `tests/test_server_describe.py` runs on MySQL and writes
`reports/mysql_nullability.md`: the columns sqlty types as non-null that MySQL marks
nullable in `CREATE TABLE ... AS` the query, and the reverse. Neither list is a defect by
itself. MySQL marks many expressions nullable that cannot be NULL, and sqlty may not prove
what MySQL does; each entry is a lead to check by hand.

    SQLTY_MYSQL_DSN=mysql://root@127.0.0.1:3306/probe \\
        uv run --with pymysql python scripts/compat/mysql_nullability.py
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT))

import pymysql  # noqa: E402
from _pytest.outcomes import Skipped  # noqa: E402

from sqlty import core  # noqa: E402
from tests.test_server_describe import MySQL, connect_mysql, corpus  # noqa: E402


def main() -> int:
    dsn = os.environ.get("SQLTY_MYSQL_DSN")
    if not dsn:
        print("SQLTY_MYSQL_DSN is not set", file=sys.stderr)
        return 2
    nullable_on_server: list[str] = []
    non_null_on_server: list[str] = []
    checked = 0
    mysql = MySQL(connect_mysql(pymysql, dsn))
    for case in corpus("mysql"):
        ours = core.analyze_select(case.query, dialect="mysql", schema_sql=case.schema_sql)
        try:
            mysql.load(case.schema_sql)
            server = mysql.describe(case.query)
        except Skipped:
            continue
        if not ours or len(ours) != len(server):
            continue
        for column, (_, server_type, nullable) in zip(ours, server, strict=True):
            if nullable is None or column.python_type in {None, "typing.Any"}:
                continue
            checked += 1
            ours_nullable = (column.python_type or "").endswith(" | None")
            python_type = str(column.python_type).replace("|", "\\|")
            entry = f"| `{case.id}` | `{column.name}` | `{python_type}` | `{server_type}` |"
            if nullable and not ours_nullable:
                nullable_on_server.append(entry)
            elif not nullable and ours_nullable:
                non_null_on_server.append(entry)
    report = [
        "# MySQL nullability against the server's",
        "",
        f"{checked} columns, `scripts/compat/mysql_nullability.py`.",
        "",
        "## Non-null in sqlty, nullable on MySQL",
        "",
        *_table(nullable_on_server),
        "## Nullable in sqlty, non-null on MySQL",
        "",
        *_table(non_null_on_server),
    ]
    (Path(__file__).parent / "reports" / "mysql_nullability.md").write_text(
        "\n".join(report).rstrip() + "\n"
    )
    print(
        f"{checked} columns: {len(nullable_on_server)} nullable on MySQL, {len(non_null_on_server)} non-null"
    )
    return 0


def _table(entries: list[str]) -> list[str]:
    if not entries:
        return ["None.", ""]
    return [
        "| Query | Column | sqlty | MySQL |",
        "|-------|--------|-------|-------|",
        *entries,
        "",
    ]


if __name__ == "__main__":
    raise SystemExit(main())
