"""Row checks in Rust (``sqlty_python::rows``) against the passes of ``sqlty._check``.

Generated modules name their row types in ``SQL[Row](...)`` calls over a small schema; both
must report the same diagnostics, at the same places. The mdtests compare the same way when
run with ``SQLTY_AST=compare``. Raise the number of generated examples with
``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import sys

import pytest
from hypothesis import given
from hypothesis import strategies as st

from sqlty import _check
from sqlty._core import row_diagnostics, wrapped_queries
from sqlty._source import ParsedSource
from tests.test_rust_types import CLASS_NAMES, FUZZ, IMPORTS, modules, parses

SCHEMA = """
CREATE TYPE mood AS ENUM ('sad', 'ok');
CREATE TYPE pair AS (a INT, b TEXT);
CREATE TABLE t (
    id INT PRIMARY KEY,
    name TEXT NOT NULL,
    note TEXT,
    score NUMERIC,
    ratio FLOAT8 NOT NULL,
    flag BOOLEAN NOT NULL,
    born DATE,
    seen TIMESTAMPTZ NOT NULL,
    feel mood NOT NULL,
    maybe mood,
    both pair,
    doc JSONB,
    tags TEXT[] NOT NULL,
    uid UUID
);
"""
COLUMNS = [
    "id",
    "name",
    "note",
    "score",
    "ratio",
    "flag",
    "born",
    "seen",
    "feel",
    "maybe",
    "both",
    "doc",
    "tags",
    "uid",
    "1",
    "'a'",
    "NULL",
    "count(*)",
    "CASE WHEN flag THEN 'a' ELSE 'b' END",
    "CASE WHEN flag THEN 'a' END",
    "CASE WHEN flag THEN 1 ELSE 2 END",
    "coalesce(note, 'x')",
    "json_build_object('a', id, 'b', note)",
    "jsonb_build_object('a', json_build_array(id, name), 'c', NULL)",
    "json_agg(json_build_object('a', id))",
    "json_build_array(id, name)",
    "to_json(name)",
    "array_agg(id)",
    "id::text",
    "TRUE",
]
NAMES = ["a", "b", "c", "d", "al", "va", "_private", "x"]
ALIASES = [
    "type Pair = tuple[int, str]\n",
    "type Pair = tuple[int, str | None]\n",
    "Pair: TypeAlias = tuple[Literal['a', 'b'], int]\n",
    "type Pair = tuple[int, ...]\n",
    "type Pair = tuple[dict[str, int], list[Row]]\n",
    "type One = int\n",
    "type One = Literal['a'] | None\n",
    "type One = Row\n",
    "type One = Again\ntype Again = Other\n",
    "type One = One\n",
    "type One = dict[str, int]\n",
    "type One = Unknown\n",
    "type One = list[int] | None\n",
    "type Pair = tuple[Row, Leaf | None]\n",
]
ROWS = [
    *CLASS_NAMES,
    "F0",
    "F1",
    "F2",
    "F3",
    "Pair",
    "One",
    "Row[int]",
    "Row[str | None]",
    "Leaf[int, str]",
    "Imported",
    "Unknown",
    "tuple[int, str]",
    "int",
]
RULES = [
    None,
    {"redundant-optional": "error", "redundant-literal": "warn"},
    {"column-count-mismatch": "ignore"},
    {"nullable-mismatch": "ignore", "column-type-mismatch": "ignore"},
    {
        "typeddict-inheritance": "ignore",
        "json-shape-mismatch": "ignore",
        "literal-mismatch": "ignore",
    },
]


@st.composite
def queries(draw: st.DrawFn) -> str:
    columns = draw(
        st.lists(st.tuples(st.sampled_from(COLUMNS), st.sampled_from(NAMES)), max_size=4)
    )
    select = ", ".join(f"{column} AS {name}" for column, name in columns) or "*"
    tail = draw(st.sampled_from(["", " WHERE note IS NOT NULL", " GROUP BY id", " LIMIT 1"]))
    query = draw(
        st.sampled_from([f"SELECT {select} FROM t{tail}", "SELECT nope FROM", "DELETE FROM t"])
    )
    return f'q = SQL[{draw(st.sampled_from(ROWS))}]("--sql\\n{query}")\n'


@st.composite
def sources(draw: st.DrawFn) -> str:
    return (
        "from sqlty import SQL\n"
        + draw(modules())
        + "".join(draw(st.lists(st.sampled_from(ALIASES), max_size=3)))
        + "".join(draw(st.lists(queries(), min_size=1, max_size=5)))
    )


FIELD_TYPES = [
    "int",
    "str",
    "str | None",
    "Optional[int]",
    "float",
    "Decimal",
    "Decimal | None",
    "bool",
    "datetime.date | None",
    "datetime.datetime",
    "uuid.UUID | None",
    "Literal['a', 'b']",
    "Literal['a']",
    "Literal['a', 'b', 'c'] | None",
    "Literal['sad', 'ok']",
    "Literal['sad'] | None",
    "Literal[1, 2]",
    "Literal[1, True, 3]",
    "dict[str, int]",
    "dict[str, Any]",
    "dict[str, int | None] | None",
    "list[Inner]",
    "list[Inner] | None",
    "Inner",
    "Inner | None",
    "Optional[Inner]",
    "list[int]",
    "list[str]",
    "list[int | str]",
    "tuple[int, str]",
    "Any",
    "object",
    "T",
    "T | None",
    "UserId",
    "Json[Inner]",
    "NotRequired[int]",
    "Annotated[str | None, Field(alias='al')]",
]
KINDS = [
    "class Row(TypedDict):",
    "class Row(TypedDict, total=False):",
    "class Row(TypedDict, closed=True):",
    "class Row(TypedDict, extra_items=int):",
    "class Row(TypedDict, extra_items=str | None):",
    "class Row[T](TypedDict):",
    "class Row(NamedTuple):",
    "class Row[T](NamedTuple):",
    "@dataclass\nclass Row:",
    "@dataclass\nclass Row[T]:",
    "class Row(BaseModel):",
    "class Row(BaseModel, strict=True):",
    "class Row(BaseModel, extra='allow'):",
    "class Row(Protocol):",
    "class Row(Inner):",
    "class Row(Inner, total=False):",
]
INNER = [
    "class Inner(TypedDict):\n    a: int\n    b: str | None\n",
    "class Inner(TypedDict):\n    a: list[int | str]\n    c: NotRequired[None]\n",
    "class Inner(BaseModel):\n    a: int\n    b: str = ''\n",
    "@dataclass\nclass Inner:\n    a: int\n    b: str\n",
    "class Inner(NamedTuple):\n    a: int\n    b: str | None\n",
    "class Inner(TypedDict):\n    a: 'Inner | None'\n",
    "class Inner(TypedDict, total=False):\n    a: str\n    a: int\n",
    "UserId = NewType('UserId', int)\nInner = dict[str, UserId]\n",
]
FIRST = (
    "from sqlty import SQL\n" + "\n".join(IMPORTS[:13]) + "\n" + "\n".join(IMPORTS[14:23]) + "\n"
)


@st.composite
def fitted(draw: st.DrawFn) -> str:
    """A row type and queries whose columns are named for its fields."""
    fields = draw(
        st.lists(st.tuples(st.sampled_from(NAMES[:4]), st.sampled_from(FIELD_TYPES)), max_size=4)
    )
    body = "".join(f"    {name}: {annotation}\n" for name, annotation in fields) or "    pass\n"
    out = [FIRST, draw(st.sampled_from(INNER)), f"{draw(st.sampled_from(KINDS))}\n{body}"]
    out += draw(st.lists(st.sampled_from(ALIASES), max_size=2))
    for _ in range(draw(st.integers(1, 3))):
        names = [name for name, _ in fields] + draw(st.lists(st.sampled_from(NAMES), max_size=1))
        names = draw(st.permutations(names)) if draw(st.booleans()) else names
        names = names[: draw(st.integers(0, len(names)))]
        select = ", ".join(f"{draw(st.sampled_from(COLUMNS))} AS {name}" for name in names) or "*"
        row = draw(
            st.sampled_from(
                [
                    *["Row", "Row", "Row[int]", "Row[str | None]", "One", "Pair", "Inner"],
                    *["Row[*Ts]", "Row[1:2]", "Row[list[int]]", "Row[(int, str)]"],
                ]
            )
        )
        out.append(f'q = SQL[{row}]("--sql\\nSELECT {select} FROM t")\n')
    return "".join(out)


def compare(
    source: str,
    rules: dict[str, str] | None = None,
    coerce: frozenset[str] = frozenset(),
    import_style: str | None = None,
    module: str = "pkg.mod",
) -> int:
    """How many diagnostics both report for ``source``; raises where they differ."""
    found = [
        q
        for q in wrapped_queries(source, import_style=import_style)
        if not q[1].startswith("tuple[")
    ]
    _check._compare_rows(  # noqa: SLF001
        ParsedSource(source), found, "postgres", SCHEMA, rules, import_style, module, coerce
    )
    rust = row_diagnostics(
        source,
        version=sys.version_info[:2],
        schema_sql=SCHEMA,
        rules=rules,
        import_style=import_style,
        module=module,
        coerce=sorted(coerce),
    )
    assert rust is not None
    return len(rust[0])


HEAD = "from sqlty import SQL\nfrom typing import Annotated, Literal, NamedTuple, TypedDict\n"
CASES = {
    "count": "class Row(TypedDict):\n    a: int\nq = SQL[Row]('--sql\\nSELECT id AS a, name AS b FROM t')\n",
    "type": "class Row(TypedDict):\n    a: str\nq = SQL[Row]('--sql\\nSELECT id AS a FROM t')\n",
    "nullable": "class Row(TypedDict):\n    a: str\nq = SQL[Row]('--sql\\nSELECT note AS a FROM t')\n",
    "position": "class Row(NamedTuple):\n    a: int\nq = SQL[Row]('--sql\\nSELECT id AS b FROM t')\n",
    "literal": (
        "class Row(TypedDict):\n    a: Literal['a']\n"
        "q = SQL[Row]('--sql\\nSELECT CASE WHEN flag THEN \\'a\\' ELSE \\'b\\' END AS a FROM t')\n"
    ),
    "json": (
        "class Row(TypedDict):\n    a: dict[str, str]\n"
        "q = SQL[Row]('--sql\\nSELECT json_build_object(\\'a\\', id) AS a FROM t')\n"
    ),
    "alias": "type Pair = tuple[int, int]\nq = SQL[Pair]('--sql\\nSELECT id, name FROM t')\n",
    "generic": (
        "class Row[T](TypedDict):\n    a: T\nq = SQL[Row[str]]('--sql\\nSELECT id AS a FROM t')\n"
    ),
    "own base": (
        "class Row(TypedDict):\n    a: int\nclass Row(Row):\n    b: str\n"
        "q = SQL[Row]('--sql\\nSELECT id AS a FROM t')\n"
    ),
    "empty literal": "type Row = Literal[()]\nq = SQL[Row]('--sql\\nSELECT id, name FROM t')\n",
    "starred argument": (
        "class Row[T](TypedDict):\n    a: T\n    v: list[T | None]\n"
        "q = SQL[Row[*Ts]]('--sql\\nSELECT name AS a, json_build_array(note) AS v, id AS z FROM t')\n"
    ),
    "argument in an attribute": (
        "class Row[T](TypedDict):\n    v: Annotated[int, T.x]\n"
        "q = SQL[Row[1]]('--sql\\nSELECT json_build_object(\\'a\\', note) AS v FROM t')\n"
    ),
    "argument as a target": (
        "class Row[T](TypedDict):\n    v: Annotated[int, [x for T in y]]\n"
        "q = SQL[Row[int | str]]('--sql\\nSELECT json_build_object(\\'a\\', note) AS v FROM t')\n"
    ),
    "deep argument": (
        f"class Row[T](TypedDict):\n    a: int\n    w: {'list[' * 120}T{']' * 120}\n"
        f"q = SQL[Row[{'list[' * 120}int{']' * 120}]]"
        "('--sql\\nSELECT name AS a, json_build_array(id) AS w FROM t')\n"
    ),
    "parameter beside a mark": (
        "class Row[T](TypedDict):\n    a: T\n    b: Annotated[str, 'T\u00bd', 'T\u0301']\n"
        "q = SQL[Row[str]]('--sql\\nSELECT name AS a, id AS b FROM t')\n"
    ),
    "composite": (
        "class Inner(TypedDict):\n    a: str\n    b: str\nclass Row(TypedDict):\n    a: Inner\n"
        "q = SQL[Row]('--sql\\nSELECT both AS a FROM t')\n"
    ),
}


@pytest.mark.parametrize("source", CASES.values(), ids=CASES.keys())
def test_cases(source: str) -> None:
    assert compare(HEAD + source) >= 1


def test_bare_wrapper_with_a_module_import() -> None:
    assert compare("import sqlty\n" + HEAD + CASES["type"], import_style="import sqlty") == 1


def test_empty_module_name() -> None:
    source = (
        "from typing import NewType\nUserId = NewType('UserId', int)\n"
        "class Row(TypedDict):\n    a: UserId | None\n"
        "q = SQL[Row]('--sql\\nSELECT id AS a FROM t')\n"
    )
    assert compare(HEAD + source, {"redundant-optional": "error"}, module="") == 1


@FUZZ
@given(
    fitted(),
    st.sampled_from(RULES),
    st.sampled_from([frozenset(), frozenset({"pkg.mod.Row", "pkg.mod.F1"})]),
)
def test_fitted_sources(source: str, rules: dict[str, str] | None, coerce: frozenset[str]) -> None:
    if parses(source):
        compare(source, rules, coerce)


@FUZZ
@given(
    sources(),
    st.sampled_from(RULES),
    st.sampled_from([frozenset(), frozenset({"pkg.mod.Row", "pkg.mod.F1"})]),
)
def test_generated_sources(
    source: str, rules: dict[str, str] | None, coerce: frozenset[str]
) -> None:
    if parses(source):
        compare(source, rules, coerce)
