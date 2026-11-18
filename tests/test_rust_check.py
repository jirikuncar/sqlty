"""The whole check of a module in Rust (``sqlty_python::check``).

Generated modules define helpers and call them, run queries on driver connections, write
``--sql`` t-strings and declare ``SQL[R, P]``. Raise the number of generated examples with
``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import sys

import pytest
from hypothesis import given
from hypothesis import strategies as st

from sqlty._core import load_schema, module_diagnostics
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


def diagnosed(
    source: str,
    dialect: str = "postgres",
    rules: dict[str, str] | None = None,
    untyped: bool = False,
    schema: bool = True,
) -> int:
    """How many diagnostics Rust reports for ``source``, -1 where it declines it."""
    found = module_diagnostics(
        source,
        version=sys.version_info[:2],
        dialect=dialect,
        schema_sql=SCHEMAS[dialect] if schema else None,
        rules=rules,
        import_style=None,
        untyped=untyped,
        module="pkg.mod",
    )
    return -1 if found is None else len(found[0])


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
    "literals": "    run('--sql\\nSELECT id FROM t', int)\n    'a' F'{n:>{s}}'\n    '' F'{n}'\n"
    "    t'{q:>3}'\n    run('SELECT nope FROM t', int)\n",
}


@pytest.mark.parametrize("call", CASES.values(), ids=CASES.keys())
def test_cases(call: str) -> None:
    driver = "aiosqlite" if "f'" in call else "psycopg"
    found = diagnosed((HEAD + HELPERS[0] + SETUP + call).replace("DRIVER", driver))
    assert found >= (driver == "psycopg")  # dynamic-query is off by default


CALLS = {
    "any row": (
        "    run(q, Annotated[Any, 1])\n    run(q, 'Annotated[Any, 1]')\n    run(q, Annotated[Any])\n",
        0,
    ),
    "key twice": (
        "    cur.execute('SELECT id FROM t WHERE id = %(a)s OR id = %(a)s', {'a': 1, 'a': b'x'})\n",
        1,
    ),
    "typed values": (
        "    cur.execute('SELECT 1 FROM b WHERE data = %s AND at = %s AND doc = %s', "
        "(b'x', datetime.time(1), 1))\n"
        "    cur.execute('SELECT 1 FROM b WHERE data = %s AND at = %s', (1, 1))\n",
        2,
    ),
    "declared fit": (
        "    cur.execute(SQL[Row, tuple[int, list[int]]]('--sql\\nSELECT id, name FROM t "
        "WHERE id = %s AND id = ANY(%s)'), (1, []))\n"
        "    cur.execute(SQL[Row, tuple[int, list[int]]]('--sql\\nSELECT id, name FROM t "
        "WHERE id = %s AND id = ANY(%s)'), (b'x', 'x'))\n",
        1,
    ),
    "declared variable": (
        "    p = SQL[Row, tuple[int, list[int]]]('--sql\\nSELECT id, name FROM t "
        "WHERE id = %s AND id = ANY(%s)')\n    cur.execute(p, (1, []))\n    cur.execute(p, (b'x', 'x'))\n",
        1,
    ),
    "number twice": ("    await pg.fetch('SELECT id FROM t WHERE id = $1 OR id = $1', b'x')\n", 1),
    "empty query": ("    cur.execute('')\n    run('', int)\n", 0),
    "not a format field": ("    cur.execute('SELECT {x.y}')\n", 0),
    "qualified any": (
        "    sqlty.SQL[Any]('--sql\\nSELECT 1')\n    x.SQL[Any]('--sql\\nSELECT 1')\n",
        0,
    ),
}


@pytest.mark.parametrize(("call", "count"), CALLS.values(), ids=CALLS.keys())
def test_calls(call: str, count: int) -> None:
    assert diagnosed((HEAD + HELPERS[0] + SETUP + call).replace("DRIVER", "psycopg")) == count


DECLARED = {
    "types": "    SQL[Row, tuple[Union[int, str], Optional[bytes], list[int], list[None], "
    "datetime.time, Annotated[Any, 1], 'Any', datetime.timedelta]]('--sql\\nSELECT id, name "
    "FROM t WHERE id = %s AND name = %s AND id = ANY(%s) AND id = ANY(%s) AND %s::TIME IS NULL "
    "AND %s::JSONB IS NULL AND %s::BYTEA IS NULL AND %s::INTERVAL IS NULL')\n",
    "named": "    class P(TypedDict):\n        a: int\n        b: bytes\n"
    "    SQL[Row, P]('--sql\\nSELECT id, name FROM t WHERE id = %(a)s AND name = %(b)s')\n",
    "shapes": "    NOSQL[a, b]\n    SQL[a, b, c]('--sql\\nSELECT 1')\n"
    "    x.SQL[Row, P]('--sql\\nSELECT 1')\n    SQL[Row, P](QUERY)\n",
    "unparsed": "    SQL[Row, tuple[int]]('--sql\\nSELEC nope %s')\n",
    "key twice": "    SQL[Row, Twice]('--sql\\nSELECT id FROM t WHERE id = %(a)s OR id = %(a)s')\n",
    "inherited": "    SQL[Row, Child]('--sql\\nSELECT id FROM t WHERE id = %(a)s AND id = %(b)s')\n",
}
DICTS = "class Twice(TypedDict):\n    a: int\n    a: str\nclass Child(Twice):\n    b: int\n"


@pytest.mark.parametrize("call", DECLARED.values(), ids=DECLARED.keys())
def test_declared_params(call: str) -> None:
    head = HEAD.replace("DRIVER", "psycopg") + "from typing import Optional, Union\n" + DICTS
    assert diagnosed(head + HELPERS[0] + SETUP.replace("DRIVER", "psycopg") + call) >= 1


def test_sqruff_of_a_module_that_does_not_parse() -> None:
    source = (
        "from sqlty import SQL\nx = (\nq = SQL[tuple[str, int]]('''--sql\n"
        "SELECT name, COUNT(*) AS n FROM users\nGROUP BY 1\nORDER BY name''')\n"
    )
    assert diagnosed(source, rules={"sqruff": "warn"}) == 2
    assert diagnosed("x = (\n", rules={"sqruff": "warn"}) == 0


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
        diagnosed(source, dialect, rules, untyped, schema)


@FUZZ
@given(fitted(), st.sampled_from(sorted(SCHEMAS)), st.booleans())
def test_generated_rows(source: str, dialect: str, untyped: bool) -> None:
    if parses(source):
        diagnosed(source, dialect, None, untyped)
