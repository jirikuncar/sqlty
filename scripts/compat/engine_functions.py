"""The built-in functions of DuckDB, SQLite, MySQL and ClickHouse, against the engines.

Each engine lists its own functions, and the queries calling them run on it:

- DuckDB: every overload of `duckdb_functions()`, called with a typed `NULL` per
  argument, and every example it lists;
- SQLite: every function of `pragma_function_list`, called with each mix of an integer,
  a real and a text column the engine accepts;
- MySQL: every `mysql> SELECT` example of the help tables (`mysql.help_topic`);
- ClickHouse: every example of `system.functions`, in process through chDB.

A column's type is what the engine describes, mapped to Python by sqlty itself: the
Python type sqlty gives a column declared with that type; an `int` is a `float`, as in
PEP 484. Writes
`reports/functions_<engine>.md`: the queries sqlty types differently (exit status 1) and
the functions it leaves untyped.

    uv run --with duckdb python scripts/compat/engine_functions.py duckdb
    uv run python scripts/compat/engine_functions.py sqlite
    SQLTY_MYSQL_DSN=mysql://root:root@127.0.0.1:3306/s \\
        uv run --with pymysql python scripts/compat/engine_functions.py mysql
    uv run --python 3.13 --with chdb python scripts/compat/engine_functions.py clickhouse
"""

from __future__ import annotations

import argparse
import functools
import itertools
import json
import os
import re
import sqlite3
import sys
from collections.abc import Callable, Iterator
from dataclasses import dataclass, field
from pathlib import Path
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT))

from sqlty import core  # noqa: E402

REPORTS = Path(__file__).parent / "reports"
NAME = re.compile(r"^\s*([A-Za-z_][A-Za-z_0-9]*)\s*\(")


@dataclass(frozen=True)
class Probe:
    function: str
    query: str


@dataclass
class Results:
    agree: int = 0
    probes: int = 0
    mismatch: list[str] = field(default_factory=list)
    untyped: dict[str, set[str]] = field(default_factory=dict)
    typed: set[str] = field(default_factory=set)
    unparsed: list[str] = field(default_factory=list)


class Engine:
    dialect: str
    title: str
    schema: str | None = None
    # (sqlty, engine) Python types that differ by design.
    TOLERATED: frozenset[tuple[str, str]] = frozenset()

    def version(self) -> str:
        raise NotImplementedError

    def probes(self) -> Iterator[Probe]:
        raise NotImplementedError

    def describe(self, query: str) -> list[str] | None:
        """The type of each result column, None when the engine rejects the query."""
        raise NotImplementedError

    def python(self, engine_type: str) -> str | None:
        return _declared(self.dialect, engine_type)


@functools.cache
def _declared(dialect: str, sql_type: str) -> str | None:
    """The Python type sqlty gives a column declared as `sql_type`."""
    try:
        columns = core.analyze_select(
            "SELECT c FROM z", dialect=dialect, schema_sql=f"CREATE TABLE z (c {sql_type} NOT NULL)"
        )
    except Exception:
        return None
    return columns[0].python_type if columns else None


LITERAL = re.compile(r"typing\.Literal\[(.*)\]")


def _parts(python_type: str | None) -> set[str]:
    """The types of a union, a literal read as its type and `None` left out."""
    parts = set()
    for part in (python_type or "").split(" | "):
        if literal := LITERAL.fullmatch(part):
            value = literal.group(1).split(", ")[0]
            part = "str" if value[0] in "'\"" else "bool" if value in ("True", "False") else "int"
        if part and part != "None":
            parts.add(part)
    return parts


class DuckDB(Engine):
    dialect = "duckdb"
    title = "DuckDB"
    SKIP = {
        "ANY",
        "ANY[]",
        "T",
        "T[]",
        "K",
        "V",
        "TABLE",
        "POINTER",
        "INVALID",
        "UNION",
        "STRUCT",
        "MAP",
        "LIST",
    }

    def __init__(self) -> None:
        import duckdb

        self.conn = duckdb.connect()
        self.module = duckdb

    def version(self) -> str:
        return self.module.__version__

    def probes(self) -> Iterator[Probe]:
        rows = self.conn.execute(
            """
            SELECT function_name, parameter_types, varargs, examples
            FROM duckdb_functions()
            WHERE function_type IN ('scalar', 'aggregate', 'macro')
              AND schema_name IN ('main', 'pg_catalog')
            ORDER BY 1
            """
        ).fetchall()
        seen: set[str] = set()
        for name, types, varargs, examples in rows:
            if not re.fullmatch(r"[a-z_][a-z_0-9]*", name):
                continue
            if types is not None and not varargs and not set(types) & self.SKIP:
                args = ", ".join(f"CAST(NULL AS {t})" for t in types)
                query = f"SELECT {name}({args}) AS r"
                if query not in seen:
                    seen.add(query)
                    yield Probe(name, query)
            for example in examples or []:
                query = f"SELECT {example} AS r"
                if query not in seen and ";" not in example:
                    seen.add(query)
                    yield Probe(name, query)

    def describe(self, query: str) -> list[str] | None:
        try:
            return [str(column[1]) for column in self.conn.execute(f"DESCRIBE {query}").fetchall()]
        except Exception:
            return None


class SQLite(Engine):
    dialect = "sqlite"
    title = "SQLite"
    schema = "CREATE TABLE v (i INTEGER NOT NULL, r REAL NOT NULL, s TEXT NOT NULL)"
    VALUES = ("i", "r", "s")
    CLASSES = {"integer": "int", "real": "float", "text": "str", "blob": "bytes"}

    def __init__(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.execute(self.schema)
        self.conn.execute("INSERT INTO v VALUES (1, 1.5, 'a')")

    def version(self) -> str:
        return sqlite3.sqlite_version

    def probes(self) -> Iterator[Probe]:
        rows = self.conn.execute(
            "SELECT DISTINCT name, narg FROM pragma_function_list WHERE builtin ORDER BY 1, 2"
        ).fetchall()
        for name, narg in rows:
            if not re.fullmatch(r"[a-z_][a-z_0-9]*", name):
                continue
            for count in [narg] if narg >= 0 else [1, 2]:
                for args in itertools.product(self.VALUES, repeat=count):
                    yield Probe(name, f"SELECT {name}({', '.join(args)}) AS r FROM v")

    def describe(self, query: str) -> list[str] | None:
        try:
            row = self.conn.execute(f"SELECT typeof(r) FROM ({query})").fetchone()
        except sqlite3.Error:
            return None
        return None if row is None or row[0] == "null" else [row[0]]

    def python(self, engine_type: str) -> str | None:
        return self.CLASSES.get(engine_type)


class MySQL(Engine):
    dialect = "mysql"
    title = "MySQL"
    # sqlty types a MySQL truth value as `bool` on purpose (docs/reference/types.md).
    TOLERATED = frozenset({("bool", "int")})
    EXAMPLE = re.compile(r"mysql> (SELECT .*?);", re.S | re.I)

    def __init__(self) -> None:
        import pymysql

        dsn = os.environ.get("SQLTY_MYSQL_DSN")
        if not dsn:
            raise SystemExit("SQLTY_MYSQL_DSN is not set")
        url = urlparse(dsn)
        self.conn = pymysql.connect(
            host=url.hostname,
            port=url.port or 3306,
            user=url.username,
            password=url.password or "",
            database=url.path.lstrip("/"),
            autocommit=True,
        )
        self.module = pymysql

    def version(self) -> str:
        with self.conn.cursor() as cursor:
            cursor.execute("SELECT VERSION()")
            return str(cursor.fetchone()[0])

    def probes(self) -> Iterator[Probe]:
        with self.conn.cursor() as cursor:
            cursor.execute("SELECT name, example FROM mysql.help_topic ORDER BY name")
            rows = cursor.fetchall()
        for topic, example in rows:
            for match in self.EXAMPLE.finditer(example.replace("\n    ->", "\n")):
                query = re.sub(r"\s+", " ", match.group(1))
                if " FROM " in query.upper().split(")")[-1] or "@" in query:
                    continue
                name = NAME.match(query[len("SELECT ") :])
                yield Probe(name.group(1).upper() if name else topic, query)

    def describe(self, query: str) -> list[str] | None:
        try:
            with self.conn.cursor() as cursor:
                cursor.execute("DROP TEMPORARY TABLE IF EXISTS probe")
                cursor.execute(f"CREATE TEMPORARY TABLE probe AS {query}")
                cursor.execute("SHOW COLUMNS FROM probe")
                return [row[1] for row in cursor.fetchall()]
        except self.module.MySQLError:
            return None


class ClickHouse(Engine):
    dialect = "clickhouse"
    title = "ClickHouse"
    QUERY = re.compile(r"```sql[^\n]*\n(.*?)```", re.S)

    def __init__(self) -> None:
        import chdb

        self.module = chdb

    def version(self) -> str:
        return self._query("SELECT version()")[0][0]

    def probes(self) -> Iterator[Probe]:
        rows = self._query(
            "SELECT name, examples FROM system.functions WHERE alias_to = '' AND NOT is_aggregate "
            "AND origin = 'System' ORDER BY name"
        )
        for name, examples in rows:
            for block in self.QUERY.findall(examples):
                for statement in block.split(";"):
                    query = " ".join(statement.split())
                    if query.upper().startswith("SELECT ") and " FROM " not in query.upper():
                        yield Probe(name, query)

    def _query(self, sql: str) -> list[list[str]]:
        text = self.module.query(sql, "JSONCompactEachRow").bytes().decode()
        return [json.loads(line) for line in text.splitlines()]

    def describe(self, query: str) -> list[str] | None:
        try:
            rows = self._query(f"DESCRIBE ({query})")
        except Exception:
            return None
        return [row[1] for row in rows]

    def python(self, engine_type: str) -> str | None:
        inner = re.fullmatch(r"(?:Nullable|LowCardinality)\((.*)\)", engine_type)
        return self.python(inner.group(1)) if inner else _declared(self.dialect, engine_type)


ENGINES: dict[str, Callable[[], Engine]] = {
    "duckdb": DuckDB,
    "sqlite": SQLite,
    "mysql": MySQL,
    "clickhouse": ClickHouse,
}


def compare(engine: Engine) -> Results:
    results = Results()
    for probe in engine.probes():
        described = engine.describe(probe.query)
        if described is None:
            continue
        results.probes += 1
        try:
            ours = core.analyze_select(
                probe.query, dialect=engine.dialect, schema_sql=engine.schema
            )
        except Exception:
            ours = None
        if not ours or len(ours) != len(described):
            results.unparsed.append(f"| `{_cell(probe.query)}` |")
            continue
        for column, engine_type in zip(ours, described, strict=True):
            expected = _parts(engine.python(engine_type))
            found = _parts(column.python_type)
            if not found:
                results.untyped.setdefault(probe.function, set()).add(engine_type)
            elif not expected:
                continue
            elif (
                not expected <= _admitted(found)
                and found != {"typing.Any"}
                and (" | ".join(sorted(found)), " | ".join(sorted(expected)))
                not in engine.TOLERATED
            ):
                results.mismatch.append(
                    f"| `{_cell(probe.query)}` | `{column.python_type}` ({column.sql_type}) "
                    f"| `{engine_type}` (`{' | '.join(sorted(expected))}`) |"
                )
            else:
                results.typed.add(probe.function)
                results.agree += 1
    return results


def report(engine: Engine, results: Results) -> str:
    untyped = sorted(set(results.untyped) - results.typed, key=str.lower)
    partly = sorted(set(results.untyped) & results.typed, key=str.lower)
    lines = [
        f"# {engine.title} {engine.version()} functions",
        "",
        f"{results.probes} queries the engine accepts, `scripts/compat/engine_functions.py "
        f"{engine.dialect}`. {results.agree} columns agree with the engine.",
        "",
        "## Typed differently",
        "",
        *_table(sorted(set(results.mismatch)), "| Query | sqlty | Engine |"),
        f"## Untyped ({len(untyped)} functions)",
        "",
        *(", ".join(f"`{name}`" for name in untyped) or "None.",),
        "",
        f"## Untyped for some arguments ({len(partly)} functions)",
        "",
        *(", ".join(f"`{name}`" for name in partly) or "None.",),
        "",
        f"## Not parsed ({len(results.unparsed)} queries)",
        "",
        *_table(sorted(set(results.unparsed))[:50], "| Query |"),
    ]
    return "\n".join(lines).rstrip() + "\n"


def _admitted(found: set[str]) -> set[str]:
    """What a value of `found` may be: an `int` where a `float` is expected (PEP 484)."""
    return found | {"int"} if "float" in found else found


def _table(rows: list[str], header: str) -> list[str]:
    if not rows:
        return ["None.", ""]
    return [header, "|" + "---|" * header.count(" | ") + "---|", *rows, ""]


def _cell(text: str) -> str:
    text = " ".join(text.split()).replace("|", "\\|")
    return text if len(text) <= 120 else text[:117] + "..."


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("engine", choices=ENGINES)
    engine = ENGINES[parser.parse_args().engine]()
    results = compare(engine)
    REPORTS.mkdir(exist_ok=True)
    out = REPORTS / f"functions_{engine.dialect}.md"
    out.write_text(report(engine, results))
    print(f"{out}: {results.agree} agree, {len(results.mismatch)} typed differently")
    return 1 if results.mismatch else 0


if __name__ == "__main__":
    raise SystemExit(main())
