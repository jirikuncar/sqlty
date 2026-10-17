"""Compare sqlty's inferred Python types with another source's, row by row.

A row is one column type (as DDL) or one expression over a typed table, with the
Python type the source expects. Each row runs through `sqlty.core.analyze_select`:

- a column: `CREATE TABLE t (c <ddl> NOT NULL)` and `SELECT c FROM t`;
- an expression: `SELECT <expr> AS x FROM t`, with `t` the row's `schema`.

The Python types are compared without `| None`; nullability is reported apart. Each
row is a match, a mismatch, a sqlty gap (sqlty leaves it untyped) or not comparable (the
source has no Python type for it).

    uv run --with sqlalchemy==2.0.43 python scripts/compat/run.py sqlalchemy
"""

from __future__ import annotations

import json
from collections import Counter
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Literal

from sqlty import core

Outcome = Literal["match", "mismatch", "sqlty-gap", "not-comparable"]


@dataclass(frozen=True)
class Row:
    source: str
    version: str
    dialect: str
    kind: Literal["column", "expression"]
    sql: str
    expected: str | None
    provenance: str
    schema: str = ""


@dataclass
class Result:
    row: Row
    sqlty: str | None
    nullable: bool | None
    outcome: Outcome


@dataclass
class Report:
    results: list[Result] = field(default_factory=list)

    def counts(self) -> Counter[str]:
        return Counter(result.outcome for result in self.results)

    def write(self, stem: Path) -> None:
        """`<stem>.json` with every row, and `<stem>.md` with the counts and mismatches."""
        stem.parent.mkdir(parents=True, exist_ok=True)
        stem.with_suffix(".json").write_text(
            json.dumps([asdict(result) for result in self.results], indent=1) + "\n"
        )
        stem.with_suffix(".md").write_text(self.markdown())

    def markdown(self) -> str:
        counts = self.counts()
        sources = sorted({(r.row.source, r.row.version) for r in self.results})
        lines = [
            f"# {', '.join(f'{s} {v}' for s, v in sources)} vs sqlty",
            "",
            "| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |",
            "|---------|------|-------|----------|-----------|----------------|",
        ]
        for dialect, kind in sorted({(r.row.dialect, r.row.kind) for r in self.results}):
            group = Counter(
                r.outcome for r in self.results if (r.row.dialect, r.row.kind) == (dialect, kind)
            )
            lines.append(
                f"| {dialect} | {kind} | {group['match']} | {group['mismatch']} "
                f"| {group['sqlty-gap']} | {group['not-comparable']} |"
            )
        lines += [
            f"| **all** | | **{counts['match']}** | **{counts['mismatch']}** "
            f"| **{counts['sqlty-gap']}** | **{counts['not-comparable']}** |",
            "",
        ]
        for outcome in ("mismatch", "sqlty-gap"):
            rows = [r for r in self.results if r.outcome == outcome]
            if not rows:
                continue
            lines += [
                f"## {outcome}",
                "",
                "| Dialect | SQL | Expected | sqlty | Provenance |",
                "|---------|-----|----------|-------|------------|",
            ]
            lines += [
                f"| {r.row.dialect} | `{r.row.sql}` | `{r.row.expected}` | `{r.sqlty}` | {r.row.provenance} |"
                for r in rows
            ]
            lines.append("")
        return "\n".join(lines)


def run(rows: list[Row]) -> Report:
    report = Report()
    for row in rows:
        if row.kind == "column":
            schema = f"CREATE TABLE t (c {row.sql} NOT NULL);"
            query = "SELECT c FROM t"
        else:
            schema = row.schema
            query = f"SELECT {row.sql} AS x FROM t" if schema else f"SELECT {row.sql} AS x"
        try:
            columns = core.analyze_select(query, dialect=row.dialect, schema_sql=schema or None)
        except RuntimeError:
            columns = None
        python = columns[0].python_type if columns else None
        sqlty = python.removesuffix(" | None") if python else None
        nullable = None if python is None else python.endswith(" | None")
        if row.expected is None:
            outcome: Outcome = "not-comparable"
        elif sqlty is None:
            outcome = "sqlty-gap"
        elif _same(sqlty, row.expected):
            outcome = "match"
        else:
            outcome = "mismatch"
        report.results.append(Result(row, sqlty, nullable, outcome))
    return report


def _same(sqlty: str, expected: str) -> bool:
    """Whether two Python types agree: a `Literal` of strings is a `str`, `Any` matches all,
    and a bare container (`list`) matches any of its parametrizations (`list[int]`)."""
    if "typing.Any" in (sqlty, expected) or sqlty == expected:
        return True
    if "[" not in expected and sqlty.split("[", 1)[0] == expected:
        return True
    # An observed value's type is one member of a union; an `int` is a valid `float`.
    if expected in sqlty.split(" | ") or (expected, sqlty) == ("int", "float"):
        return True
    if sqlty.startswith("typing.Literal["):
        literal = sqlty.removeprefix("typing.Literal[")
        return expected == ("str" if literal[:1] in "'\"" else "int")
    return False
