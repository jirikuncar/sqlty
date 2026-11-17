"""Write the inference rules of DuckDB's built-in functions, read from DuckDB itself.

    uv run python scripts/generate_duckdb_rules.py            # rewrite the rules
    uv run python scripts/generate_duckdb_rules.py --check    # exit 1 if out of date

`make generate-duckdb-rules` runs it after a DuckDB upgrade; `tests/test_duckdb_types.py`
fails until then.

Every overload of `duckdb_functions()` with concrete parameter types is called with a
typed `NULL` per argument, and its result type is what DuckDB describes: the catalog's
`return_type` is not always what binding gives (`date_part('year', ts)` is a `BIGINT`,
not the `DOUBLE` it lists). A function some hand-written rule already names is left to
that rule, and so is one whose overloads disagree on a result sqlty cannot tell apart.

Writes `rules/duckdb/catalog.inference.toml`, one rule per function and result, and
`tests/mdtest/duckdb/catalog.md`, which asserts each rule. Nullability: an aggregate is
NULL over no rows unless DuckDB says otherwise; a scalar function NULL for a NULL argument
is `strict` when a call with values gives a value and its name is not one that may find
nothing (`try_*`, `*extract*`, ...), else `nullable`.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

import duckdb

from sqlty import core

ROOT = Path(__file__).resolve().parent.parent
RULES = ROOT / "rust" / "crates" / "sqlty-sql" / "rules"
OUTPUT = RULES / "duckdb" / "catalog.inference.toml"
MDTEST = ROOT / "tests" / "mdtest" / "duckdb" / "catalog.md"
COMPILER = RULES / "compile.rs"

NAME = re.compile(r"[a-z][a-z0-9_]*")
SKIPPED_NAME = re.compile(r"^(collate_|icu_collate_|__|pg_|st_|variant_)")
# Parsed as expressions of their own, which the analyzer types.
PARSED = {
    "substring",
    "substr",
    "position",
    "overlay",
    "trim",
    "extract",
    "ceil",
    "ceiling",
    "floor",
}
MAY_FIND_NOTHING = re.compile(
    r"^try_|extract|^json_|element_at|position|indexof|^nullif$|getvariable|current_setting|"
    r"^(list|array)_(min|max|first|last|any_value|aggr|aggregate|reduce)|^map_|^struct_|^union_"
)
SAMPLES = {
    "BOOLEAN": "true",
    "DATE": "2024-01-02",
    "TIME": "10:00:00",
    "TIMESTAMP": "2024-01-02 10:00:00",
    "TIMESTAMP WITH TIME ZONE": "2024-01-02 10:00:00+00",
    "TIME WITH TIME ZONE": "10:00:00+00",
    "INTERVAL": "1 day",
    "VARCHAR": "a",
    "BLOB": "a",
    "UUID": "00000000-0000-0000-0000-000000000001",
    "BIT": "101",
}
NUMBERS = {
    "TINYINT",
    "SMALLINT",
    "INTEGER",
    "BIGINT",
    "HUGEINT",
    "UTINYINT",
    "USMALLINT",
    "UINTEGER",
    "UBIGINT",
    "UHUGEINT",
    "FLOAT",
    "DOUBLE",
    "DECIMAL",
}
# Families a parameter type admits, as the analyzer types arguments: DuckDB casts an
# argument up within its family.
FAMILIES = {
    "INTEGER_FAMILY": ("@integer", "@unsigned", "INT128", "UINT128"),
    "FLOAT_FAMILY": ("FLOAT", "DOUBLE", "REAL"),
    "TEXT_FAMILY": ("@text",),
}


@dataclass(frozen=True)
class Overload:
    name: str
    kind: str
    params: tuple[str, ...]


def sql_types() -> set[str]:
    block = COMPILER.read_text().split("const SQL_TYPES", 1)[1].split("];", 1)[0]
    return set(re.findall(r'"([A-Z0-9]+)"', block)) - {"ARRAY"}


def ruled_names() -> set[str]:
    """Functions a hand-written rule names, which apply to DuckDB."""
    names: set[str] = set()
    for path in RULES.rglob("*.inference.toml"):
        if path == OUTPUT or path.parent.name not in ("common", "duckdb"):
            continue
        for table in tomllib.loads(path.read_text()).get("function", []):
            names.update(
                n.lower() for n in [table.get("function"), *table.get("functions", [])] if n
            )
    return names


def canonical(duckdb_type: str, known: set[str]) -> str | None:
    """The SQL type sqlty gives a column of `duckdb_type`, when a rule can name it."""
    columns = core.analyze_select(
        "SELECT c FROM z", dialect="duckdb", schema_sql=f"CREATE TABLE z (c {duckdb_type} NOT NULL)"
    )
    if not columns or columns[0].python_type is None or columns[0].sql_type is None:
        return None
    sql = re.sub(r"\(.*\)$", "", columns[0].sql_type)
    inner = re.fullmatch(r"ARRAY<(\w+)>", sql)
    if inner and inner.group(1) in known:
        return sql
    return sql if sql in known else None


def family(duckdb_type: str, known: set[str]) -> str | None:
    """The predicate key of a parameter: its family, else its SQL type."""
    base = re.sub(r"\(.*\)$", "", duckdb_type)
    if base in NUMBERS - {"FLOAT", "DOUBLE", "DECIMAL"}:
        return "INTEGER_FAMILY"
    if base in ("FLOAT", "DOUBLE"):
        return "FLOAT_FAMILY"
    if base == "VARCHAR":
        return "TEXT_FAMILY"
    sql = canonical(duckdb_type, known)
    return "ARRAY" if sql and sql.startswith("ARRAY<") else sql


def literal(duckdb_type: str) -> str | None:
    base = re.sub(r"\(.*\)$", "", duckdb_type)
    if base in NUMBERS:
        return f"CAST(1 AS {duckdb_type})"
    if base in SAMPLES:
        return f"CAST('{SAMPLES[base]}' AS {duckdb_type})"
    return None


def describe(conn: duckdb.DuckDBPyConnection, query: str) -> str | None:
    try:
        return str(conn.execute(f"DESCRIBE {query}").fetchall()[0][1])
    except duckdb.Error:
        return None


def value(conn: duckdb.DuckDBPyConnection, query: str) -> tuple[bool, object]:
    try:
        return True, conn.execute(query).fetchall()[0][0]
    except duckdb.Error:
        return False, None


def overloads(conn: duckdb.DuckDBPyConnection, skip: set[str]) -> list[Overload]:
    rows = conn.execute(
        """
        SELECT DISTINCT function_name, function_type,
            CASE WHEN varargs IS NULL THEN parameter_types ELSE list_append(parameter_types, varargs) END
        FROM duckdb_functions()
        WHERE function_type IN ('scalar', 'aggregate') AND schema_name = 'main'
          AND NOT has_side_effects AND stability <> 'VOLATILE'
        ORDER BY 1, 3
        """
    ).fetchall()
    return [
        Overload(name, kind, tuple(params))
        for name, kind, params in rows
        if NAME.fullmatch(name) and not SKIPPED_NAME.match(name) and name not in skip | PARSED
    ]


@dataclass
class Rule:
    name: str
    result: str
    nullability: str
    keys: list[tuple[str, ...]]
    by: int
    example: str


def rules(conn: duckdb.DuckDBPyConnection) -> tuple[list[Rule], list[str]]:
    """The rules of every function whose overloads sqlty can tell apart by the types of
    their first two arguments, and the names of those it cannot."""
    known = sql_types()
    by_name: dict[str, list[tuple[Overload, str | None]]] = defaultdict(list)
    for overload in overloads(conn, ruled_names()):
        result = None
        if overload.params and all(
            canonical(p, known) or family(p, known) for p in overload.params
        ):
            args = ", ".join(f"CAST(NULL AS {p})" for p in overload.params)
            described = describe(conn, f"SELECT {overload.name}({args})")
            result = described and canonical(described, known)
        by_name[overload.name].append((overload, result))
    out: list[Rule] = []
    dropped: list[str] = []
    for name, typed in sorted(by_name.items()):
        if all(result is None for _, result in typed):
            continue
        groups = None
        for by in (0, 1, 2):
            keyed: dict[tuple[str, ...], tuple[str | None, Overload]] = {}
            for overload, result in typed:
                key = tuple(
                    (family(p, known) or f"?{p}") if i < len(overload.params) else "?absent"
                    for i, p in enumerate(
                        overload.params[:by] + ("",) * (by - len(overload.params))
                    )
                )
                if key in keyed and keyed[key][0] != result:
                    break
                keyed[key] = (result, overload)
            else:
                groups = {
                    key: (result, overload)
                    for key, (result, overload) in keyed.items()
                    if result is not None and not any(k.startswith("?") for k in key)
                }
                break
        if not groups:
            dropped.append(name)
            continue
        merged: dict[tuple[str, str, tuple[str, ...]], Rule] = {}
        for key, (result, overload) in groups.items():
            nullability = _nullability(conn, overload)
            example = _call(overload, lambda p: literal(p) or f"CAST(NULL AS {p})")
            rule = merged.setdefault(
                (result, nullability, key if by == 2 else ()),
                Rule(name, result, nullability, [], by, example),
            )
            rule.keys.append(key)
        out.extend(merged.values())
    return out, dropped


def _call(overload: Overload, render) -> str:
    return f"{overload.name}({', '.join(render(p) for p in overload.params)})"


def _nullability(conn: duckdb.DuckDBPyConnection, overload: Overload) -> str:
    nulls = _call(overload, lambda p: f"CAST(NULL AS {p})")
    if overload.kind == "aggregate":
        ok, empty = value(conn, f"SELECT {nulls} FROM range(0)")
        return "non-null" if ok and empty is not None else "aggregate"
    if not all(literal(p) for p in overload.params):
        return "nullable"
    ok, from_null = value(conn, f"SELECT {nulls}")
    ran, from_values = value(conn, f"SELECT {_call(overload, literal)}")
    if ok and from_null is None and ran and from_values is not None:
        return "nullable" if MAY_FIND_NOTHING.search(overload.name) else "strict"
    return "nullable"


def predicate(key: str) -> list[str]:
    return list(FAMILIES.get(key, (key,)))


def toml(rules: list[Rule], version: str) -> str:
    lines = [
        f"# DuckDB {version} built-in functions, as `duckdb_functions()` lists them and DuckDB types",
        "# their calls. Generated by scripts/generate_duckdb_rules.py; do not edit.",
    ]
    counts: dict[str, int] = defaultdict(int)
    for rule in rules:
        counts[rule.name] += 1
        suffix = f"-{counts[rule.name]}" if counts[rule.name] > 1 else ""
        lines += [
            "",
            "[[function]]",
            f'id = "duckdb.catalog.{rule.name.replace("_", "-")}{suffix}"',
            f'function = "{rule.name.upper()}"',
            'dialects = ["duckdb"]',
        ]
        if rule.by == 1:
            args = sorted({t for key in rule.keys for t in predicate(key[0])})
            lines.append(f"argument = {_list(args)}")
        elif rule.by == 2:
            if len(rule.keys) > 1:
                raise ValueError(f"{rule.name}: several argument pairs give {rule.result}")
            first, second = rule.keys[0]
            lines.append(f"arguments = [{_list(predicate(first))}, {_list(predicate(second))}]")
        lines += [
            f'type = "{rule.result}"',
            f'nullability = "{rule.nullability}"',
            f'reference.duckdb = "DuckDB {version} duckdb_functions(): {rule.name}"',
            f'reasoning = "DuckDB describes {_example(rule)} as {rule.result}."',
        ]
    return "\n".join(lines) + "\n"


def _example(rule: Rule) -> str:
    return rule.example.replace('"', "'")


def _list(items: list[str]) -> str:
    return "[" + ", ".join(f'"{item}"' for item in items) + "]"


def mdtest(rules: list[Rule]) -> str:
    out = [
        "# DuckDB built-in functions",
        "",
        "Generated by `scripts/generate_duckdb_rules.py`; do not edit. A call of each",
        "function `rules/duckdb/catalog.inference.toml` types.",
        "",
        "```toml",
        "[tool.sqlty]",
        'dialect = "duckdb"',
        "```",
    ]
    counts: dict[str, int] = defaultdict(int)
    for rule in rules:
        counts[rule.name] += 1
        suffix = f"-{counts[rule.name]}" if counts[rule.name] > 1 else ""
        rule_id = f"duckdb.catalog.{rule.name.replace('_', '-')}{suffix}"
        out += [
            "",
            "```sql",
            f"SELECT {rule.example} AS r",
            f"-- rules: r: {rule_id} / {rule_id}",
            "```",
        ]
    return "\n".join(out) + "\n"


def generate() -> tuple[dict[Path, str], list[str]]:
    """The text of each file, and the functions left out."""
    found, dropped = rules(duckdb.connect())
    return {OUTPUT: toml(found, duckdb.__version__), MDTEST: mdtest(found)}, dropped


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="exit 1 if the files are out of date")
    check = parser.parse_args().check
    outputs, dropped = generate()
    if check:
        stale = [
            path for path, text in outputs.items() if not path.exists() or path.read_text() != text
        ]
        for path in stale:
            print(f"{path.relative_to(ROOT)} is out of date", file=sys.stderr)
        return 1 if stale else 0
    for path, text in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    rules_written = outputs[OUTPUT].count("[[function]]")
    print(
        f"{rules_written} rules; left out, overloads sqlty cannot tell apart: {', '.join(dropped)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
