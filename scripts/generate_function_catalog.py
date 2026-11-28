"""Write the signatures of each dialect's built-in functions, read from its documentation.

    uv run python scripts/generate_function_catalog.py postgres
    uv run --with duckdb==2.0.0 python scripts/generate_function_catalog.py duckdb

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

DuckDB: `duckdb_functions()` of the version the rules cite, with an overload per arity, and
`varargs` as optional repeated arguments, `[, type, ...]`. More than four overloads with the
same parameter names are written as one, `ANY` where their types differ. A macro's untyped
parameter is `ANY`. A table function returns `TABLE`; `duckdb_functions()` lists the parameters
it takes by name (`read_csv(path, header = true)`) after the positional ones, so only as many
are written as DuckDB binds positionally.
The page is the documentation page of the function's first category, or of its kind.
"""

from __future__ import annotations

import argparse
import html
import re
import urllib.request
from pathlib import Path
from typing import Any

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


DUCKDB_VERSION = "2.0.0"
DUCKDB_DOCS = "https://duckdb.org/docs/2.0/sql/functions/"
DUCKDB_SOURCE = f"duckdb_functions() of DuckDB {DUCKDB_VERSION}"
DUCKDB_PAGES = {
    "aggregate": "aggregates",
    "array": "array",
    "bitstring": "bitstring",
    "blob": "blob",
    "date": "date",
    "geometry": "geometry",
    "lambda": "lambda",
    "list": "list",
    "numeric": "numeric",
    "regex": "regular_expressions",
    "string": "text",
    "text_similarity": "text",
    "timestamp": "timestamp",
    "variant": "variant",
    "window": "window_functions",
}
DUCKDB_KINDS = ("scalar", "aggregate", "window", "macro", "table", "table_macro")
# More overloads than this with the same parameter names are one, typed where they agree.
DUCKDB_OVERLOADS = 4
DUCKDB_NAME = re.compile(r"[a-z][a-z0-9_]*")
DUCKDB_NO_MATCH = ("No function matches", "does not support the supplied arguments")


def duckdb_positional(con: Any, name: str, types: list[str]) -> int:
    """How many of a table function's parameters a call binds positionally, the rest by name."""
    import duckdb  # noqa: PLC0415

    def binds(count: int) -> bool:
        args = ", ".join("NULL" if "ANY" in t else f"CAST(NULL AS {t})" for t in types[:count])
        try:
            con.execute(f"DESCRIBE SELECT * FROM {name}({args})")
        except duckdb.Error as error:
            return not any(message in str(error) for message in DUCKDB_NO_MATCH)
        return True

    return max((count for count in range(len(types) + 1) if binds(count)), default=len(types))


def duckdb_catalog() -> list[str]:
    import duckdb  # noqa: PLC0415  # only this dialect needs it

    if duckdb.__version__ != DUCKDB_VERSION:
        raise SystemExit(f"DuckDB {DUCKDB_VERSION} expected, found {duckdb.__version__}")
    con = duckdb.connect()
    rows = con.execute(
        """
        SELECT function_name, function_type, parameters, parameter_types, varargs, return_type,
            categories
        FROM duckdb_functions()
        WHERE schema_name IN ('main', 'pg_catalog') AND function_type IN ?
        ORDER BY function_name, len(parameters), function_oid
        """,
        [list(DUCKDB_KINDS)],
    ).fetchall()
    groups: dict[tuple[object, ...], list[tuple[list[str], str, str]]] = {}
    for name, kind, names, types, varargs, returns, categories in rows:
        if not DUCKDB_NAME.fullmatch(name) or name.startswith(("__", "pg_")):
            continue
        types = [(t or "ANY").replace('"NULL"', "ANY") for t in (types or [None] * len(names))]
        result = "TABLE" if kind in ("table", "table_macro") else (returns or "ANY")
        if result == "TABLE":
            names = names[: duckdb_positional(con, name, types)]
            types = types[: len(names)]
        result = result.replace('"NULL"', "ANY")
        page = next((DUCKDB_PAGES[c] for c in categories if c in DUCKDB_PAGES), None)
        page = page or DUCKDB_PAGES.get(kind, "overview")
        key = (name, kind in ("table", "table_macro"), tuple(names), varargs)
        groups.setdefault(key, []).append((types, result, page))
    lines: dict[str, str] = {}
    for (name, _, names, varargs), overloads in groups.items():
        if len(overloads) > DUCKDB_OVERLOADS:
            merged = [{types[i] for types, _, _ in overloads} for i in range(len(names))]
            results = {result for _, result, _ in overloads}
            overloads = [
                (
                    [kinds.pop() if len(kinds) == 1 else "ANY" for kinds in merged],
                    results.pop() if len(results) == 1 else "ANY",
                    overloads[0][2],
                )
            ]
        for types, result, page in overloads:
            params = ", ".join(f"{n}: {t}" for n, t in zip(names, types, strict=True))
            if varargs:
                params += f"[{', ' if params else ''}{varargs}, ...]"
            lines.setdefault(f"{name}({params}) -> {result}", f"{page}.html")
    return [f"{line}\t{doc}" for line, doc in lines.items()]


def render(dialect: str, docs: str, source: str, lines: list[str]) -> str:
    header = [
        f"# Built-in functions of {dialect}, from {source}.",
        "# Generated by scripts/generate_function_catalog.py; do not edit.",
        f"# docs: {docs}",
    ]
    return "\n".join(header + sorted(lines, key=lambda line: line.split("(", 1)[0])) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("dialect", choices=["postgres", "duckdb"])
    args = parser.parse_args()
    if args.dialect == "postgres":
        text = render("postgres", POSTGRES_DOCS, POSTGRES_SOURCE, postgres())
    else:
        text = render("duckdb", DUCKDB_DOCS, DUCKDB_SOURCE, duckdb_catalog())
    (OUTPUT / f"{args.dialect}.txt").write_text(text)


if __name__ == "__main__":
    main()
