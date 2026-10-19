"""The functions and operators of the PostgreSQL documentation, against a live server.

Prepares three sets of queries on the server and compares the types it describes with
`core.analyze_select`:

- every example of the function and operator tables of the "Functions and Operators"
  chapter (`expression → result`), for the server's major version;
- every overload, in `pg_proc`, of a function the chapter documents, called with a typed
  `NULL` per argument (`int4` for a polymorphic one);
- every binary and prefix operator of `pg_operator`, likewise.

Writes `reports/pg_docs.md`: the queries sqlty types differently (exit status 1), leaves
untyped, or can't parse.

    SQLTY_PG_DSN=postgresql://postgres:postgres@localhost:5432/postgres \\
        uv run python scripts/compat/pg_docs.py

The pages are fetched from postgresql.org, or read from a local copy of the HTML
documentation with `--docs DIR`.
"""

from __future__ import annotations

import argparse
import html
import os
import re
import sys
import urllib.request
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT))

import psycopg  # noqa: E402
from psycopg import pq  # noqa: E402

from sqlty import core  # noqa: E402
from tests.test_pg_describe import compare, load_pg_types  # noqa: E402

PAGE = re.compile(r'href="(functions-[a-z0-9-]+\.html)"')
EXAMPLE = re.compile(r'<p>\s*<code class="literal">((?:(?!</code>).)*)</code>\s*→', re.S)
FUNCTION = re.compile(r'<code class="function">([a-z_0-9]+)</code>')
TAG = re.compile(r"<[^>]+>")

# A polymorphic argument is called with an int4, int4[] or int4 range.
POLYMORPHIC = {
    "anyelement": "int4",
    "anynonarray": "int4",
    "anycompatible": "int4",
    "anycompatiblenonarray": "int4",
    '"any"': "int4",
    "anyarray": "int4[]",
    "anycompatiblearray": "int4[]",
    "anyrange": "int4range",
    "anycompatiblerange": "int4range",
    "anymultirange": "int4multirange",
    "anycompatiblemultirange": "int4multirange",
}
# Types no query can produce from a NULL, or that only internal callers pass.
PSEUDO = {
    "internal",
    "cstring",
    "record",
    "trigger",
    "event_trigger",
    "anyenum",
    "language_handler",
}

FUNCTIONS = """
SELECT p.proname, p.prokind, array(SELECT format_type(t, NULL) FROM unnest(p.proargtypes::oid[]) t)
FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
WHERE n.nspname = 'pg_catalog' AND p.prokind IN ('f', 'a', 'w') AND p.provariadic = 0
    AND p.proname = ANY(%s)
ORDER BY 1, 3
"""
OPERATORS = """
SELECT o.oprname, format_type(o.oprleft, NULL), format_type(o.oprright, NULL)
FROM pg_operator o JOIN pg_namespace n ON n.oid = o.oprnamespace
WHERE n.nspname = 'pg_catalog' AND o.oprright <> 0
ORDER BY 1, 2, 3
"""


@dataclass(frozen=True)
class Probe:
    source: str
    query: str


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--docs", type=Path, help="a local copy of the HTML documentation")
    docs: Path | None = parser.parse_args().docs
    dsn = os.environ.get("SQLTY_PG_DSN")
    if not dsn:
        print("SQLTY_PG_DSN is not set", file=sys.stderr)
        return 2
    results: dict[str, list[str]] = {"mismatch": [], "untyped": [], "unparsed": [], "rejected": []}
    agree = 0
    with psycopg.connect(dsn, autocommit=True) as conn:
        conn.execute("SET statement_timeout = '5s'")
        major = conn.info.server_version // 10000
        types = load_pg_types(conn)
        pages = _pages(docs, major)
        probes = [*_examples(pages), *_functions(conn, pages), *_operators(conn)]
        for probe in probes:
            server = _describe(conn, probe.query)
            entry = f"| {probe.source} | `{_cell(probe.query)}`"
            if isinstance(server, str):
                if probe.source.endswith(".html"):
                    results["rejected"].append(f"{entry} | {_cell(server)} |")
                continue
            ours = core.analyze_select(probe.query, dialect="postgres")
            if not ours or len(ours) != len(server):
                results["unparsed"].append(f"{entry} |")
                continue
            for column, (name, oid) in zip(ours, server, strict=True):
                server_type, kind = types.get(oid, (str(oid), "b"))
                if column.python_type is None and column.sql_type is None:
                    results["untyped"].append(f"{entry} | `{server_type}` |")
                elif why := compare(column, name, server_type, kind):
                    results["mismatch"].append(f"{entry} | {_cell(why)} |")
                else:
                    agree += 1
    report = [
        f"# PostgreSQL {major} functions and operators",
        "",
        f"{len(probes)} queries, `scripts/compat/pg_docs.py`: the examples of the documentation's "
        "function and operator tables, each `pg_proc` overload of a documented function, and each "
        f"`pg_operator` operator. {agree} columns agree with the server.",
        "",
        "## Typed differently",
        "",
        *_table(results["mismatch"], "Difference"),
        "## Untyped",
        "",
        *_table(results["untyped"], "Server type"),
        "## Not parsed",
        "",
        *_table(results["unparsed"]),
        "## Examples rejected by the server",
        "",
        "The example needs a table, type or role the documentation creates elsewhere.",
        "",
        *_table(results["rejected"], "Error"),
    ]
    (Path(__file__).parent / "reports" / "pg_docs.md").write_text("\n".join(report).rstrip() + "\n")
    print(
        ", ".join(f"{len(entries)} {kind}" for kind, entries in results.items())
        + f", {agree} agree"
    )
    return 1 if results["mismatch"] else 0


def _pages(docs: Path | None, major: int) -> list[tuple[str, str]]:
    """The name and HTML of each page of the chapter."""
    if docs is not None:
        return [(path.name, path.read_text()) for path in sorted(docs.glob("functions-*.html"))]
    base = f"https://www.postgresql.org/docs/{major}/"
    return [
        (page, _fetch(base + page))
        for page in sorted(set(PAGE.findall(_fetch(base + "functions.html"))))
    ]


def _examples(pages: list[tuple[str, str]]) -> Iterator[Probe]:
    for page, text in pages:
        for match in EXAMPLE.finditer(text):
            expression = " ".join(html.unescape(TAG.sub("", match.group(1))).split()).rstrip(";")
            if expression.lower().startswith("select "):
                yield Probe(page, expression)
            else:
                yield Probe(page, f"SELECT {expression}")


def _functions(conn: Any, pages: list[tuple[str, str]]) -> Iterator[Probe]:
    documented = sorted({name for _, text in pages for name in FUNCTION.findall(text)})
    for name, kind, arguments in conn.execute(FUNCTIONS, (documented,)).fetchall():
        if PSEUDO & set(arguments):
            continue
        typed = ", ".join(f"NULL::{POLYMORPHIC.get(argument, argument)}" for argument in arguments)
        yield Probe("pg_proc", f"SELECT {name}({typed}){' OVER ()' if kind == 'w' else ''}")


def _operators(conn: Any) -> Iterator[Probe]:
    for operator, left, right in conn.execute(OPERATORS).fetchall():
        operands = [
            POLYMORPHIC.get(operand, operand) for operand in (left, right) if operand != "-"
        ]
        if PSEUDO & set(operands):
            continue
        typed = [f"NULL::{operand}" for operand in operands]
        yield Probe(
            "pg_operator",
            f"SELECT {typed[0]} {operator} {typed[1]}"
            if len(typed) == 2
            else f"SELECT {operator} {typed[0]}",
        )


def _fetch(url: str) -> str:
    with urllib.request.urlopen(url, timeout=30) as response:  # noqa: S310
        text: str = response.read().decode()
    return text


def _describe(conn: Any, query: str) -> list[tuple[str, int]] | str:
    """The names and type OIDs of the query's columns, or the server's error."""
    pgconn = conn.pgconn
    result = pgconn.prepare(b"sqlty_docs", query.encode())
    if result.status != pq.ExecStatus.COMMAND_OK:
        return str(result.error_message.decode(errors="replace").strip().splitlines()[0])
    try:
        description = pgconn.describe_prepared(b"sqlty_docs")
        return [
            (description.fname(i).decode(), description.ftype(i))
            for i in range(description.nfields)
        ]
    finally:
        pgconn.exec_(b"DEALLOCATE sqlty_docs")


def _cell(text: str) -> str:
    return text.replace("|", "\\|").replace("`", "'")


def _table(entries: list[str], *columns: str) -> list[str]:
    if not entries:
        return ["None.", ""]
    header = ["Source", "Query", *columns]
    return [
        "| " + " | ".join(header) + " |",
        "|" + "|".join("---" for _ in header) + "|",
        *entries,
        "",
    ]


if __name__ == "__main__":
    raise SystemExit(main())
