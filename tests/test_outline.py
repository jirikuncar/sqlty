"""The outline type extraction reads, built in Rust (``sqlty._core.module_outline``), against ``ast``.

Each case and generated module must dump the same as ``ast.parse`` cut by ``_outline``,
positions included, with its classes in ``ParsedSource.classes`` order; the type definitions
read from both must be equal. Raise the number of generated examples with
``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import ast
import os
import pathlib
import sys
import typing

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

import sqlty
import sqlty._source
from sqlty._core import module_outline
from sqlty._source import ParsedSource, _breadth_first, _outline, _Scan
from sqlty._type_extractor import extract_type_definitions
from sqlty._type_extractor.info import TypeInfo

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))


def same(text: str) -> None:
    tree = ast.parse(text)
    built = module_outline(text)
    assert built is not None, text
    module, classes = built
    expected = ast.dump(_outline(tree), include_attributes=True, indent=1)
    assert ast.dump(module, include_attributes=True, indent=1) == expected, text
    scan = _Scan()
    scan.visit(tree)
    assert [c.lineno for c in classes] == [c.lineno for c in _breadth_first(scan.classes)]


def types(text: str, mode: str, monkeypatch: pytest.MonkeyPatch) -> dict[str, object]:
    monkeypatch.setattr(sqlty._source, "AST", mode)
    found = extract_type_definitions(ParsedSource(text))
    return {name: _summary(info) for name, info in found.items()}


def _summary(info: TypeInfo) -> dict[str, object]:
    def plain(value: object) -> object:
        if isinstance(value, ast.AST):
            return ast.dump(value, include_attributes=True)
        if isinstance(value, dict):
            return {k: plain(v) for k, v in value.items()}
        return repr(value)

    return {
        slot: plain(getattr(info, slot, None)) for slot in type(info).__slots__ if slot != "coerced"
    }


CASES = {
    "f-strings": 'x = f"{a!r}" f"{ b = !s:>10}" f"{c:{d}.{e}f}" f\'{"q"}\' rf"\\d{x}" F"{{}}{x}{{"\n',
    "multiline f-string with a comment": 'x = f"""\n{a\n # c\n}\n"""\n',
    "f-string specs and debug text": 'x = f"{x:=10}" f"{(x:=10)}" f"{x!r:}" f"{x=:}" f"{x=!a}"\n',
    "nested specs": 'x = f"{x:{y:{z}}}"\n',
    "f-strings merged with plain and u strings": (
        'x = f"{\'a\' if b else \'c\'}" f"{x!r}" "plain" f"{y}" u"z"\n'
        'x = u"a" f"{b}"\ny = f"{b}" u"a"\nz = f""\nw = f"" ""\n'
    ),
    "t-strings": (
        't = t"{x!r:>{w}} {y=} {z:{a}}"\nu = t"a" t"b{c}" t"""\n{d}\n"""\n'
        'x = t"{ x  !r }"\nz = t"{ (x) }"\n'
    ),
    "escapes": 'x = "\\N{BULLET}" "\\777" r"\\N" u"\\u00e9"\ny = b"\\x00\\777" rb"\\x" Br"c"\n',
    "continuation lines": 'x = "a" \\\n    "b"\ny = """\\\nab"""\n',
    "crlf": 'x = """a\r\nb"""\r\nclass A:\r\n    y: int = 1\r\n',
    "lone cr": 'x = 1\ry = "a\\\r"\rclass A:\r    y: int\r',
    "form feed and tabs": "x = 1\x0c\nif x:\n\tclass A:\n\t\ty: int\n",
    "non-ascii columns": 'ž = "é"; ñ = f"{ž}é{ñ!r}"\nclass Ž:\n    é: "ñ" = "ü"\n',
    "decorators with comments": "@ d . e ( 1 )\n# c\n\n@(f)\nasync  def g(): pass\n@d\nclass A: pass\n",
    "parenthesized bases and annotations": (
        "class A(\n  B,\n  ):\n  x: (\n    int\n  ) = (1)\n"
        'def f(a: "int" = 1, *args: int, b: (\n    str\n) = "", **kw: (int)): pass\n'
    ),
    "parameters": "def f(*, a): ...\ndef g(a, /): ...\ndef h(*a: *Ts): ...\nlambda a, b=1, *c, d, e=2, **f: 0\n",
    "type parameters": "type X[T] = list[T]\ndef f[T: (int, str), *Ts = *tuple[int], **P = [int]](): pass\n",
    "calls and generators": "f(a)(b)[c].d\nf(*a, **b, c=1)\nf(x for x in y if z)\nf((x for x in y))\n",
    "slices": "a[1:2, ::3, ...]\na[:]\na[1:]\na[b:=1]\na[*b]\n",
    "operators": "x = not a and b or c if d else e\nx = -~+a ** -b\nx = a < b <= c is not d not in e\n",
    "displays": 'x = [*a, *b]\n{**a, "b": 1}\n{1, 2}\n()\n(a,)\na, *b = c\n[a, b] = c\n',
    "comprehensions": (
        "x = [i for i in range(3) for j in range(i) if i if j]\nx = {k: v for k, v in d}\n"
    ),
    "numbers": "x = 0b1_0 + 0o7 + 0xFF + 1_000 + 1e-3 + 1J + 00 + 9999999999999999999999 + .5 + 5.\n",
    "annotated assignments": "x: int\nx.y: int = 1\n(x): int = 1\nx[0]: int\n",
    "imports": "import a.b.c as d, e\nfrom ... import x\nfrom .a import (b as c, d)\nfrom a import *\n",
    "match": (
        "match x:\n    case {1: a, **r}: pass\n    case [a, *_]: pass\n    case P(a, b=c) if d: pass\n"
        '    case -1 | 1+2j | "s" | b"b" | a.b: pass\n    case (a as b): pass\n    case True: pass\n'
    ),
    "try": "try:\n    pass\nexcept A, B:\n    pass\nfinally:\n    pass\n",
    "try star": "try:\n    pass\nexcept* C:\n    pass\n",
    "with": "with (a as b, c as d,):\n    pass\nwith (a, b):\n    pass\n",
    "function bodies are cut to the classes they hold": (
        "def f():\n    x = 1\n    if a:\n        pass\n    elif b:\n        class C: pass\n    else:\n"
        "        x = 1\n    for i in x:\n        try:\n            class D: pass\n        except E:\n"
        "            pass\n    match x:\n        case 1:\n            class F: pass\n        case _:\n"
        "            pass\n"
        "class A:\n    def f(self):\n        x = 1\n        class B: pass\n    async def g(self): return 1\n"
    ),
    "statements a file may hold": (
        "del a, b[0], c.d\nglobal a, b\nassert x, y\nraise A from B\nreturn\nyield 1\nawait x\n"
        "while x:\n    pass\nelse:\n    pass\nasync def f():\n    async with a as b: pass\n"
        "    async for x in y: pass\n    [x async for x in y]\n"
    ),
}


@pytest.mark.parametrize("text", CASES.values(), ids=CASES.keys())
def test_cases(text: str) -> None:
    same(text)


PY315 = {
    "lazy imports": "lazy import f\nlazy from a import b\n",
    "unpacking in comprehensions": "x = {**d for d in ds}\nx = [*a for a in b]\n",
}


@pytest.mark.parametrize("text", PY315.values(), ids=PY315.keys())
def test_syntax_new_in_315(text: str) -> None:
    if sys.version_info >= (3, 15):
        same(text)
    else:
        with pytest.raises(SyntaxError):
            ast.parse(text)
        assert module_outline(text) is None


DECLINED = {
    "surrogate escape": 'from typing import TypedDict\nx = "\\ud800"\nclass A(TypedDict):\n    a: int\n',
    "longer than 256 KiB": "from typing import TypedDict\nclass A(TypedDict):\n    a: int\n"
    + "x = 1\n" * 50_000,
    "nested too deep": "from typing import TypedDict\nx = "
    + "-" * 1001
    + "1\nclass A(TypedDict):\n    a: int\n",
}


@pytest.mark.parametrize("text", DECLINED.values(), ids=DECLINED.keys())
def test_declined_files_have_no_types(text: str, monkeypatch: pytest.MonkeyPatch) -> None:
    """Where Rust declines a file ``ast`` reads, there is no fallback: it has no types."""
    assert module_outline(text) is None
    assert types(text, "python", monkeypatch)
    assert types(text, "rust", monkeypatch) == {}


def test_own_sources() -> None:
    for path in pathlib.Path(sqlty.__file__).parent.rglob("*.py"):
        same(path.read_text(encoding="utf-8"))


ALPHABET = st.sampled_from(
    ["a", " ", "é", "ž", "\\n", "\\\\", "\\x41", "\\N{BULLET}", "'", '"', "#", "\t"]
)
FIELDS = st.sampled_from(
    [
        "{x}",
        "{x!r}",
        "{x=}",
        "{ x = !s:^5}",
        "{x:>{w}}",
        "{x:{y}.{z}f}",
        "{{",
        "}}",
        "{'q'}",
        "{x:%Y-%m}",
    ]
)


@st.composite
def strings(draw: st.DrawFn) -> str:
    parts = []
    for _ in range(draw(st.integers(1, 3))):
        prefix = draw(st.sampled_from(["", "u", "r", "f", "rf", "F", "Rf"]))
        quote = draw(st.sampled_from(["'", '"', "'''", '"""']))
        pieces = draw(st.lists(FIELDS if "f" in prefix.lower() else ALPHABET, max_size=4))
        body = "".join(pieces).replace(quote[0], "\\" + quote[0])
        parts.append(f"{prefix}{quote}{body}{quote}")
    return " ".join(parts)


@st.composite
def templates(draw: st.DrawFn) -> str:
    return " ".join(
        f"t'{''.join(draw(st.lists(FIELDS, max_size=3)))}'" for _ in range(draw(st.integers(1, 2)))
    )


ANNOTATIONS = st.sampled_from(
    [
        "int",
        "str | None",
        "list[int]",
        "Annotated[int, Field(gt=0)]",
        "NotRequired[str]",
        "'Other'",
        "dict[str, list[tuple[int, ...]]]",
        "ClassVar[int]",
        "Literal['a', \"b\"]",
        "(\n        int\n    )",
    ]
)


@st.composite
def classes(draw: st.DrawFn) -> list[str]:
    base = draw(
        st.sampled_from(
            ["TypedDict", "NamedTuple", "BaseModel", "Protocol", "str", "TypedDict, total=False"]
        )
    )
    decorator = draw(st.sampled_from(["", "@dataclass\n", "@dataclass(frozen=True)  # c\n"]))
    lines = [f"{decorator}class {draw(st.sampled_from(['Row', 'Other', 'É']))}({base}):"]
    for i in range(draw(st.integers(0, 4))):
        default = draw(
            st.one_of(
                st.none(),
                strings(),
                templates(),
                st.sampled_from(["1", "Field(default=f'{x}')", "None"]),
            )
        )
        annotation = draw(ANNOTATIONS)
        lines.append(f"    f{i}: {annotation}" + ("" if default is None else f" = {default}"))
    if draw(st.booleans()):
        lines.append("    model_config = ConfigDict(extra='forbid')")
    if draw(st.booleans()):
        lines += ["    def m(self):", f"        return {draw(strings())}"]
    if len(lines) == 1:
        lines.append("    pass")
    return lines


statements = st.one_of(
    classes(),
    st.builds(lambda s: [f"X = {s}"], strings()),
    st.builds(lambda s: [f"Y = {s}"], templates()),
    st.builds(lambda a: [f"Alias = {a}"], ANNOTATIONS),
    st.just(['R = TypedDict("R", {"a": int, "b": str}, total=False)']),
    st.just(["U = NewType('U', int)", "type T = list[U]"]),
)


def _nest(body: list[str], kind: str) -> list[str]:
    inner = [f"    {line}" for line in body]
    match kind:
        case "def":
            return ["def f():", "    x = 1", *inner]
        case "if":
            return ["if c:", "    pass", "elif d:", *inner, "else:", "    y = 2"]
        case "try":
            return ["try:", *inner, "except E:", "    pass"]
    return body


blocks = st.recursive(
    statements,
    lambda inner: st.builds(_nest, inner, st.sampled_from(["def", "if", "try", "none"])),
    max_leaves=4,
)


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=list(HealthCheck))
@given(st.lists(blocks, min_size=1, max_size=5), st.sampled_from(["\n", "\r\n", "\r"]))
def test_generated_modules(blocks: list[list[str]], newline: str) -> None:
    header = [
        "from typing import Annotated, ClassVar, Literal, NamedTuple, NewType, NotRequired, Protocol, TypedDict",
        "from dataclasses import dataclass",
        "from pydantic import BaseModel, ConfigDict, Field",
    ]
    text = newline.join(header + [line for block in blocks for line in block]) + newline
    try:
        ast.parse(text)
    except SyntaxError:
        return
    same(text)
    with pytest.MonkeyPatch.context() as monkeypatch:
        assert types(text, "rust", monkeypatch) == types(text, "python", monkeypatch)


def test_function_bodies_hold_no_names(monkeypatch: pytest.MonkeyPatch) -> None:
    """A name bound only in a function body, cut from the outline, is not a module binding."""
    text = "if c:\n    def f():\n        Id = str\nId = int\nclass R(TypedDict):\n    a: Id\n"
    text = "from typing import TypedDict\n" + text
    rust = types(text, "rust", monkeypatch)
    assert rust == types(text, "python", monkeypatch)
    assert typing.cast(dict[str, object], rust["R"])["field_types"] == {"a": repr(int)}
