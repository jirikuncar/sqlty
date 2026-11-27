"""Write the signatures of each dialect's built-in functions, read from its documentation.

    uv run python scripts/generate_function_catalog.py postgres

Writes `rust/crates/sqlty-sql/functions/<dialect>.txt`, one overload per line:

    left(string: text, n: integer) -> text	functions-string.html#FUNCTIONS-STRING-OTHER

A parameter is `name: type`, or `type` when the documentation names none; `[, ...]` holds
optional parameters and `...` repeats the parameter before it. Overloads keep the order
of the documentation. After the tab, the page of
the documentation, relative to the `# docs:` URL of the header. `build.rs` compiles the
files into `sqlty_sql::signatures`.

PostgreSQL: the function tables of `doc/src/sgml/func.sgml` at the tag of the version the
rules cite. A signature written with keywords (`substring(s FROM n)`, `trim(BOTH ...)`)
is SQL syntax, not a call, and is left out.
"""

from __future__ import annotations

import argparse
import html
import re
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "rust" / "crates" / "sqlty-sql" / "functions"

POSTGRES_TAG = "REL_17_12"
POSTGRES_SOURCE = (
    f"https://raw.githubusercontent.com/postgres/postgres/{POSTGRES_TAG}/doc/src/sgml/func.sgml"
)
POSTGRES_DOCS = "https://www.postgresql.org/docs/17/"
# Words a signature may hold that are not SQL syntax: what follows an aggregate's arguments
# and a parameter's default.
ORDER_BY = re.compile(r"\s*<literal>ORDER BY</literal>\s*<literal>input_sort_columns</literal>")
# Functions the documentation gives as a synopsis, without a result type: SQL syntax that
# reads as a call.
POSTGRES_SYNOPSES = {
    "coalesce(value: anyelement, ...) -> anyelement": "#FUNCTIONS-COALESCE-NVL-IFNULL",
    "nullif(value1: anyelement, value2: anyelement) -> anyelement": "#FUNCTIONS-NULLIF",
    "greatest(value: anyelement, ...) -> anyelement": "#FUNCTIONS-GREATEST-LEAST",
    "least(value: anyelement, ...) -> anyelement": "#FUNCTIONS-GREATEST-LEAST",
}
# Aggregates whose result the documentation gives as "same as input type".
POSTGRES_AGGREGATES = {
    "any_value(anyelement) -> anyelement",
    "max(anyelement) -> anyelement",
    "min(anyelement) -> anyelement",
}
DEFAULT = re.compile(r"\s*<literal>DEFAULT</literal>\s*<literal>[^<]*</literal>")


def text(markup: str) -> str:
    return html.unescape(re.sub(r"<[^>]+>", "", markup)).strip()


def postgres_signature(markup: str) -> str | None:
    """`name(params) -> result` of a `func_signature` paragraph, `None` for SQL syntax."""
    markup = re.sub(r"<indexterm.*?</indexterm>", "", markup, flags=re.S)
    markup = re.sub(r"\s+", " ", markup).strip()
    found = re.match(r"<function>(\w+)</function> \((.*?)\) <returnvalue>(.*)$", markup)
    if not found:
        return None
    name, params, result = found.groups()
    params = DEFAULT.sub("", ORDER_BY.sub("", params))
    params = params.replace("<optional>", "[").replace("</optional>", "]")
    for literal in re.findall(r"<literal>([^<]*)</literal>", params):
        if literal not in ("VARIADIC", "*"):
            return None
    params = re.sub(r"<literal>VARIADIC</literal>\s*", "VARIADIC ", params)
    params = params.replace("<literal>*</literal>", "*")
    params = re.sub(
        r"<parameter>([^<]+)</parameter>\s*<(type|replaceable)>([^<]+)</\2>", r"\1: \3", params
    )
    params = re.sub(r"<(type|replaceable)>([^<]+)</\1>", r"\2", params)
    if any(c in params for c in "<{("):
        return None
    params = re.sub(r"\s*\[\s*,?\s*\.\.\.\s*\]", ", ...", params)
    params = re.sub(r"\s*\[\s*,\s*", "[, ", params)
    params = re.sub(r"\s*\[\s*", "[", params)
    params = re.sub(r"\s*\]", "]", params)
    params = re.sub(r"\s*,\s*", ", ", params).strip()
    params = params.replace("[, , ...]", ", ...").replace("[, ...]", ", ...")
    result = text(result.split("</returnvalue>", 1)[0] + result.split("</returnvalue>", 1)[1])
    result = re.sub(r"\s+", " ", result).replace(" ( ", "(").replace(" )", ")")
    result = result.replace(" [, ... ]", "[, ...]")
    if not result or "same as" in result or "see text" in params:
        return None
    return f"{name.lower()}({params}) -> {result}"


def postgres() -> list[str]:
    with urllib.request.urlopen(POSTGRES_SOURCE) as response:
        source = response.read().decode()
    lines: dict[str, str] = {}
    page = anchor = ""
    for token in re.finditer(
        r'<sect1 id="([^"]+)"|<table id="([^"]+)"|<para role="func_signature">(.*?)</para>',
        source,
        re.S,
    ):
        sect1, table, signature = token.groups()
        if sect1:
            page, anchor = f"{sect1}.html", ""
        elif table:
            anchor = f"#{table.upper()}"
        elif signature and (line := postgres_signature(signature)):
            lines.setdefault(line, f"{page}{anchor}")
    for line, anchor in POSTGRES_SYNOPSES.items():
        lines[line] = f"functions-conditional.html{anchor}"
    for line in POSTGRES_AGGREGATES:
        lines[line] = "functions-aggregate.html#FUNCTIONS-AGGREGATE-TABLE"
    return [f"{line}\t{doc}" for line, doc in lines.items()]


def write(dialect: str, docs: str, source: str, lines: list[str]) -> None:
    header = [
        f"# Built-in functions of {dialect}, from {source}.",
        "# Generated by scripts/generate_function_catalog.py; do not edit.",
        f"# docs: {docs}",
    ]
    OUTPUT.mkdir(exist_ok=True)
    (OUTPUT / f"{dialect}.txt").write_text(
        "\n".join(header + sorted(lines, key=lambda line: line.split("(", 1)[0])) + "\n"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("dialect", choices=["postgres"])
    args = parser.parse_args()
    if args.dialect == "postgres":
        write("postgres", POSTGRES_DOCS, POSTGRES_SOURCE, postgres())


if __name__ == "__main__":
    main()
