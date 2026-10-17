"""The comparison harness of scripts/compat/, on hand-written rows."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "compat"))

from harness import Row, run  # noqa: E402


def row(sql: str, expected: str | None, kind: str = "column", schema: str = "") -> Row:
    return Row("test", "1", "postgres", kind, sql, expected, "here", schema)  # type: ignore[arg-type]


def test_outcomes(tmp_path: Path) -> None:
    report = run(
        [
            row("INTEGER", "int"),
            row("NUMERIC(10, 2)", "float"),
            row("INT4RANGE", "psycopg.types.range.Range"),
            row("TEXT", None),
            row("JSONB", "dict"),
            row("i + 1", "int", "expression", "CREATE TABLE t (i INTEGER NOT NULL);"),
            row("array_agg(i)", "list", "expression", "CREATE TABLE t (i INTEGER NOT NULL);"),
        ]
    )
    assert [r.outcome for r in report.results] == [
        "match",
        "mismatch",
        "sqlty-gap",
        "not-comparable",
        "match",
        "match",
        "match",
    ]
    assert report.results[0].nullable is False
    report.write(tmp_path / "report")
    markdown = (tmp_path / "report.md").read_text()
    assert "| postgres | column | 2 | 1 | 1 | 1 |" in markdown
    assert "`NUMERIC(10, 2)` | `float` | `decimal.Decimal`" in markdown
    assert (tmp_path / "report.json").read_text().startswith("[")
