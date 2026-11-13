"""``sqlty_python::json_types`` writes the ``TypedDict``s ``sqlty._json_types`` writes."""

from __future__ import annotations

import os
import sys

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from sqlty._core import add_json_types as rust_json_types
from sqlty._json_types import add_json_types

VERSION = sys.version_info[:2]
EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "200"))
SCHEMA = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, tags JSONB, born DATE NOT NULL);"


def same(source: str, **options: str) -> str:
    """What both write for ``source``, and for their own output again."""
    python = add_json_types(source, schema_sql=SCHEMA, **options)
    assert rust_json_types(source, version=VERSION, schema_sql=SCHEMA, **options) == python, source
    again = add_json_types(python, schema_sql=SCHEMA, **options)
    assert rust_json_types(python, version=VERSION, schema_sql=SCHEMA, **options) == again, python
    return python


def query(sql: str, row: str = "tuple[typing.Any]", wrapper: str = "SQL") -> str:
    return f'{wrapper}[{row}]("--sql\\n{sql}")'


OBJECT = "jsonb_build_object('n', 1, 'tags', jsonb_agg(name), 'who', jsonb_build_object('id', id))"

CASES = [
    f"import typing\nfrom sqlty import SQL\nq = {query(f'SELECT {OBJECT} AS doc FROM users')}\n",
    f"import typing\nfrom sqlty import SQL\nq = {query(f'SELECT {OBJECT} FROM users')}\n",
    f"from typing import Any\nfrom sqlty import SQL\nq = {query('SELECT to_jsonb(u) AS u FROM users u', 'tuple[Any]')}\n",
    f"import typing\nfrom sqlty import SQL\nDoc = 1\nq = {query(f'SELECT {OBJECT} AS doc FROM users')}\n",
    f"import typing\nfrom sqlty import SQL\nq = {query(f'SELECT {OBJECT} AS doc FROM users')}\nimport os\n",
    f"import typing\r\nfrom sqlty import SQL\r\nq = {query(f'SELECT {OBJECT} AS doc FROM users')}\r\n",
    f"import typing\nfrom sqlty import SQL\nq = {query(f'SELECT {OBJECT} AS doc FROM users')}",
    f"import typing\nfrom sqlty import SQL\nq = {query(f'SELECT {OBJECT} AS doc FROM users')}; import os",
    f"q = {query(f'SELECT {OBJECT} AS doc FROM users')}\n",
    f"import typing\nimport sqlty\nq = {query(f'SELECT {OBJECT} AS doc FROM users', wrapper='sqlty.SQL')}\n",
    "import typing\nfrom sqlty import SQL\n"
    f"q = {query('SELECT 1::INTEGER AS a, jsonb_build_object() AS b', 'tuple[int, typing.Any]')}\n",
    "import typing\nfrom sqlty import SQL\n"
    + f"q = {query('''SELECT jsonb_build_object('class', 1, 'a b', 2, 'é', 3, 'match', 4) AS d''')}\n",
    "import typing\nfrom sqlty import SQL\n"
    + f"q = {query('''SELECT jsonb_build_object('x', '{}'::jsonb, 'y', '[]'::jsonb, 'z', NULL) AS d''')}\n",
    "import typing\nfrom sqlty import SQL\n"
    + f"q = {query('''SELECT jsonb_object_agg(name, jsonb_build_object('n', id)) AS d FROM users''')}\n",
    f"import typing\nfrom sqlty import SQL\né = 'é'\nq = {query(f'SELECT {OBJECT} AS doc FROM users')}  # é\n",
    f"import typing\nfrom sqlty import SQL\nq = (\n    {query(f'SELECT {OBJECT} AS doc FROM users')},\n"
    f"    {query(f'SELECT {OBJECT} AS doc FROM users')},\n    {query('SELECT 1')},\n)\n",
    "def f(:\n",
    "",
]


@pytest.mark.parametrize("source", CASES)
def test_cases(source: str) -> None:
    same(source)


def test_a_class_is_written_once() -> None:
    source = same(CASES[0])
    assert source.count("# sqlty: generated") == 2
    assert same(source) == source
    reformatted = "import typing\nfrom sqlty import SQL\nq = " + query(
        "SELECT jsonb_build_object('a b', 1) AS doc"
    )
    written = same(reformatted + "\n")
    assert "Doc = typing.TypedDict('Doc', {'a b': int})" in written
    # A formatter's double quotes are still the class that was written.
    double = written.replace("'Doc', {'a b': int}", '"Doc", {"a b": int}').replace(
        "tuple[Doc]", "tuple[typing.Any]"
    )
    assert same(double).count("# sqlty: generated") == 1


def test_classes_are_named_in_source_order() -> None:
    inner = query("SELECT jsonb_build_object('a', 1) AS doc")
    outer = query("SELECT jsonb_build_object('b', 1) AS doc")
    source = f"import typing\nfrom sqlty import SQL\ndef f():\n    return {inner}\nq = {outer}\n"
    written = rust_json_types(source, version=VERSION)
    assert "return SQL[tuple[Doc]]" in written and "q = SQL[tuple[Doc2]]" in written
    # `ast.walk` reaches the module-level query first.
    assert "q = SQL[tuple[Doc]]" in add_json_types(source)


KEY = st.sampled_from(["n", "id", "a b", "class", "é", "Item", "x1", "1x", "", "_", "match"])


@st.composite
def shapes(draw: st.DrawFn, depth: int = 0) -> str:
    kinds = ["1", "1.5", "'a'", "true", "NULL", "name", "id", "tags", "born"]
    if depth < 3:
        kinds += ["object"] * 3 + ["array", "agg", "map", "literal"]
    kind = draw(st.sampled_from(kinds))
    if kind == "object":
        keys = draw(st.lists(KEY, max_size=3, unique=True))
        fields = ", ".join(f"'{key}', {draw(shapes(depth + 1))}" for key in keys)
        return f"jsonb_build_object({fields})"
    if kind == "array":
        return f"jsonb_build_array({draw(shapes(depth + 1))})"
    if kind == "agg":
        return f"jsonb_agg({draw(shapes(depth + 1))})"
    if kind == "map":
        return f"jsonb_object_agg(name, {draw(shapes(depth + 1))})"
    if kind == "literal":
        return draw(st.sampled_from(["'{}'::jsonb", "'[1]'::jsonb", "'{\"a\": [1.5]}'::jsonb"]))
    return kind


ALIAS = st.sampled_from(["doc", "Doc", "order_lines", "x", "1", "a b", "é", "class"])


@st.composite
def modules(draw: st.DrawFn) -> str:
    lines = ["import typing", "from sqlty import SQL"]
    # `ast.walk` reads the calls breadth first and Rust in source order, which names the
    # classes alike only when the queries are equally deep.
    shape = draw(st.sampled_from(["{}", "def f():\n    return {}", "{}  # é", "é = [{}]"]))
    bare = draw(st.booleans())
    for _ in range(draw(st.integers(1, 3))):
        columns = draw(st.lists(st.tuples(shapes(), ALIAS), min_size=1, max_size=3))
        select = ", ".join(f'{json} AS \\"{alias}\\"' for json, alias in columns)
        row = ", ".join(draw(st.sampled_from(["typing.Any", "typing.Any", "int"])) for _ in columns)
        statement = f"q = {query(f'SELECT {select} FROM users GROUP BY id', f'tuple[{row}]')}"
        lines.insert(
            draw(st.integers(0, len(lines))),
            shape.format(statement.removeprefix("q = ") if bare else statement),
        )
    for extra in draw(
        st.lists(
            st.sampled_from(
                ["Doc = 1", "class DocItem: ...", "import os", "del X", "def X(): ..."]
            ),
            max_size=2,
        )
    ):
        lines.insert(draw(st.integers(0, len(lines))), extra)
    eol = draw(st.sampled_from(["\n", "\n", "\r\n"]))
    return eol.join(lines).replace("\n", eol).replace("\r\r", "\r") + draw(
        st.sampled_from([eol, ""])
    )


@settings(max_examples=EXAMPLES, deadline=None)
@given(modules())
def test_generated_modules(source: str) -> None:
    same(source)
