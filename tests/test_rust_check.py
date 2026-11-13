"""The whole check of a module in Rust (``sqlty_python::check``) against ``sqlty._check``.

Generated modules define helpers and call them, run queries on driver connections, write
``--sql`` t-strings and declare ``SQL[R, P]``; both must report the same diagnostics. The
mdtests compare the same way when run with ``SQLTY_AST=compare``. Raise the number of
generated examples with ``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import pytest
from hypothesis import given
from hypothesis import strategies as st

from sqlty import _check
from sqlty._core import load_schema
from sqlty._source import ParsedSource
from tests.test_rust_rows import SCHEMA, fitted
from tests.test_rust_types import FUZZ, parses

HEAD = """\
import datetime, decimal, uuid
import DRIVER, sqlite3, asyncpg
import sqlty
from typing import Annotated, Any, Literal, NamedTuple, TypedDict, overload
from sqlty import SQL, NoParams
"""
HELPERS = [
    "def run[T](q: Annotated[str, SQL], t: type[T], params=None) -> list[T]: ...\n",
    "def run[T](q: Annotated[str, SQL], t: type[T], /, *args) -> T: ...\n",
    "def run(q: 'Annotated[str, SQL]', **params): ...\n",
    "def run(sql: Annotated[str, sqlty.SQL], parameters=()): ...\n",
    "def run(*, q: Annotated[str, SQL], t: 'type[T]' = int, args=()) -> 'T | None': ...\n",
    "@overload\ndef run(q: Annotated[str, SQL]) -> None: ...\n@overload\n"
    "def run[T](q: Annotated[str, SQL], t: type[T]) -> T: ...\n",
    "def run(q: Annotated[str, Other], t=None): ...\n",
    "def outer(q: Annotated[str, SQL]):\n    return run(q, int)\n",
    "def other(text, q: Annotated[str, SQL] = 'SELECT 1', *params): ...\n",
    "class Db:\n"
    "    def one[T](self, q: Annotated[str, SQL], t: type[T], params=()) -> T: ...\n"
    "    @staticmethod\n    def st(q: Annotated[str, SQL], *args): ...\n"
    "    @classmethod\n    def cl[T](cls, q: Annotated[str, SQL], t: type[T], **params) -> T: ...\n"
    "    def use(self):\n        self.db = Db()\n"
    "        return self.one('SELECT id, name FROM t WHERE id = %s', Row, (1,)), "
    "self.db.one('SELECT nope FROM t', int)\n",
    "db = Db()\nrepo: Db = make()\n",
]
QUERIES = [
    "SELECT id, name FROM t",
    "SELECT id FROM t WHERE id = %s",
    "SELECT id, note FROM t WHERE name = %s AND id > %s",
    "SELECT name FROM t WHERE id = %(a)s AND note = %(b)s",
    "SELECT name FROM t WHERE id = $1 AND born < $2",
    "SELECT name FROM t WHERE id = ? AND flag = ?",
    "SELECT name FROM t WHERE id = :a",
    "INSERT INTO t (id, name, note) VALUES (%s, %s, %s)",
    "INSERT INTO t (id, name) VALUES (%(a)s, %(b)s) RETURNING id",
    "UPDATE t SET note = %s WHERE id = ANY(%s)",
    "SELECT * FROM t",
    "SELECT nope FROM t",
    "SELECT id FROM missing",
    "SELECT id FROM t WHERE seen > $1 - INTERVAL '1 day'",
    "SELECT %s::text, score FROM t WHERE score = %s",
    "SELECT {col} FROM t",
    "SELEC",
    "",
    "--sql\\nSELECT id FROM t WHERE id = %s",
    "-- sqlty: db=other\\nSELECT 1",
]
VALUES = [
    "1",
    "-1",
    "'x'",
    "None",
    "1.5",
    "True",
    "b'x'",
    "[1, 2]",
    "(1, 'a')",
    "[]",
    "datetime.date(2020, 1, 1)",
    "datetime.datetime.now()",
    "uuid.UUID('12345678123456781234567812345678')",
    "decimal.Decimal(1)",
    "n",
    "s",
    "unknown",
    "f'{n}'",
    "*rest",
    "int(s)",
]
ROWS = ["int", "Row", "tuple[int, str]", "Any", "'Any'", "list[int]", "Row | None", "Pair", "x"]


@st.composite
def arguments(draw: st.DrawFn) -> str:
    values = draw(st.lists(st.sampled_from(VALUES), max_size=3))
    match draw(st.integers(0, 7)):
        case 0:
            return ""
        case 1:
            return ", " + ", ".join(values) if values else ""
        case 2:
            return f", ({''.join(f'{v}, ' for v in values)})"
        case 3:
            return f", [{', '.join(values)}]"
        case 4:
            return (
                ", {" + ", ".join(f"'{k}': {v}" for k, v in zip("abc", values, strict=False)) + "}"
            )
        case 5:
            return "".join(f", {k}={v}" for k, v in zip("abc", values, strict=False))
        case 6:
            return f", params=({''.join(f'{v}, ' for v in values)})"
    return f", {draw(st.sampled_from(VALUES))}"


@st.composite
def calls(draw: st.DrawFn) -> str:
    query = draw(st.sampled_from(QUERIES))
    quoted = draw(st.sampled_from([f'"{query}"', f'"""{query}"""', "QUERY", f'f"{query}"', "q"]))
    row = draw(st.sampled_from(ROWS))
    args = draw(arguments())
    line = draw(
        st.sampled_from(
            [
                f"run({quoted}, {row}{args})",
                f"run({quoted}{args})",
                f"run(q={quoted}, t={row}{args})",
                f"run(sql={quoted}{args})",
                f"other(1, {quoted}{args})",
                f"db.one({quoted}, {row}{args})",
                f"repo.one({quoted}, {row}{args})",
                f"Db.st({quoted}{args})",
                f"Db.cl({quoted}, {row}{args})",
                f"Db.one({quoted}, {row}{args})",
                f"cur.execute({quoted}{args})",
                f"conn.execute({quoted}{args})",
                f"cur.executemany({quoted}, [{args[2:]}])",
                f"lite.execute({quoted}{args})",
                f"lite.executemany({quoted}{args})",
                f"await pg.fetch({quoted}{args})",
                f"await pg.fetchrow({quoted}{args})",
                f"SQL[{row}]({quoted})",
                f"cur.execute(SQL[{row}]({quoted}){args})",
                f"SQL[{row}, tuple[int, str]]({quoted})",
                f"SQL[{row}, tuple[int | None]]({quoted})",
                f"SQL[{row}, tuple[str, ...]]({quoted})",
                f"SQL[{row}, P]({quoted})",
                f"SQL[{row}, NoParams]({quoted})",
                f"SQL[{row}, sqlty.NoParams]({quoted})",
                f't"--sql\\n{query.replace("%s", "{n}").replace("--sql", "")}"',
                f't"--sql\\nSELECT id FROM t WHERE name = {{s}} AND id = {{{draw(st.sampled_from(VALUES[:-4]))}:s}}"',
            ]
        )
    )
    return f"    x = {line}\n"


PARAMS = [
    "class P(TypedDict):\n    a: int\n    b: str | None\n",
    "class P(TypedDict, total=False):\n    a: int\n",
    "class P(TypedDict):\n    a: datetime.date\n    c: bool\n",
    "class P(NamedTuple):\n    a: int\n",
    "",
]
SETUP = """\
QUERY = "SELECT id FROM t"
type Pair = tuple[int, str]
class Row(TypedDict):
    id: int
    name: str
conn = DRIVER.connect("")
cur = conn.cursor()
lite = sqlite3.connect("")
async def main(n: int, s: str, q: str, pg: asyncpg.Connection, *rest):
"""


@st.composite
def sources(draw: st.DrawFn) -> str:
    source = (
        HEAD
        + "".join(draw(st.lists(st.sampled_from(HELPERS), max_size=4, unique=True)))
        + draw(st.sampled_from(PARAMS))
        + SETUP
        + "".join(draw(st.lists(calls(), min_size=1, max_size=6)))
    )
    # Without psycopg, a helper call whose query is not a literal is read in Rust too.
    return source.replace("DRIVER", draw(st.sampled_from(["psycopg", "aiosqlite"])))


SCHEMAS = {dialect: load_schema(SCHEMA, dialect=dialect) for dialect in ("postgres", "sqlite")}
RULES = [
    None,
    {"param-type-mismatch": "ignore"},
    {"param-count-mismatch": "ignore", "extra-param": "error", "sqruff": "warn"},
    {"dynamic-query": "error", "unchecked-query": "error", "select-star": "error"},
    {"unparsed-query": "ignore", "unknown-column": "ignore", "params-type-mismatch": "ignore"},
]


def compare(
    source: str,
    dialect: str = "postgres",
    rules: dict[str, str] | None = None,
    untyped: bool = False,
    schema: bool = True,
) -> int:
    """How many diagnostics both report for ``source``, -1 where Rust leaves it to Python."""
    parsed = ParsedSource(source)
    usage = _check.RowUsage()
    settings = {
        "dialect": dialect,
        "schema_sql": SCHEMAS[dialect] if schema else None,
        "rules": rules,
        "import_style": None,
        "untyped": untyped,
        "module": "pkg.mod",
    }
    found = _check._diagnostics(  # noqa: SLF001
        parsed, helpers=None, row_usage=usage, variants=None, coerce=frozenset(), **settings
    )
    rust = _check._module_diagnostics(  # noqa: SLF001
        source, version=_check.sys.version_info[:2], **settings
    )
    _check._compare_module(parsed, found, usage, coerce=frozenset(), **settings)  # noqa: SLF001
    return -1 if rust is None else len(found)


CASES = {
    "count": "    cur.execute('SELECT id FROM t WHERE id = %s')\n",
    "type": "    cur.execute('SELECT id FROM t WHERE id = %s', (b'x',))\n",
    "single": "    cur.execute('SELECT id FROM t WHERE id = %s', 1)\n",
    "extra": "    cur.execute('SELECT id FROM t WHERE id = %(a)s', {'a': 1, 'z': 2})\n",
    "helper row": "    run('SELECT id, name FROM t', int)\n",
    "helper params": "    run('SELECT id FROM t WHERE id = %s', int, (b'x',))\n",
    "dynamic": "    run(f'SELECT {n}', int)\n",
    "declared": "    SQL[Row, tuple[int, str]]('--sql\\nSELECT id, name FROM t WHERE id = %s')\n",
    "names": "    run('SELECT nope FROM t', int)\n",
    "unparsed": "    run('SELEC', int)\n",
    "template": '    t"--sql\\nSELECT id FROM t WHERE id = {s}"\n',
}


@pytest.mark.parametrize("call", CASES.values(), ids=CASES.keys())
def test_cases(call: str) -> None:
    driver = "aiosqlite" if "f'" in call else "psycopg"
    found = compare((HEAD + HELPERS[0] + SETUP + call).replace("DRIVER", driver))
    assert found >= (driver == "psycopg")  # dynamic-query is off by default


@FUZZ
@given(
    sources(),
    st.sampled_from(sorted(SCHEMAS)),
    st.sampled_from(RULES),
    st.booleans(),
    st.booleans(),
)
def test_generated_calls(
    source: str, dialect: str, rules: dict[str, str] | None, untyped: bool, schema: bool
) -> None:
    if parses(source):
        compare(source, dialect, rules, untyped, schema)


@FUZZ
@given(fitted(), st.sampled_from(sorted(SCHEMAS)), st.booleans())
def test_generated_rows(source: str, dialect: str, untyped: bool) -> None:
    if parses(source):
        compare(source, dialect, None, untyped)
