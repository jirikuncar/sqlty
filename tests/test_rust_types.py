"""Row types read in Rust (``sqlty_python::types``) against ``sqlty._type_extractor``.

Every module of the test suite and its docs, and generated ones, must give the same row
types, field for field, and the same answers on how their field types relate; expressions
must unparse as ``ast.unparse`` writes them. Raise the number of generated examples with
``SQLTY_FUZZ_EXAMPLES``.
"""

from __future__ import annotations

import ast
import itertools
import os
import pathlib
import re
import sys
import typing
import warnings

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty._core import type_definitions, type_relations, unparse_expression
from sqlty._type_extractor import (
    admits_none,
    check_type_compatibility,
    column_literal_values,
    declared_literal_values,
    extract_type_definitions,
    field_accepts,
    field_type,
    is_optional,
    parse_tuple_annotation,
)
from sqlty._type_extractor.info import PYTHON_TYPE_MAP
from sqlty._type_extractor.subtype import (
    _LAX_FROM,
    _consistent,
    _erase,
    _is_subtype,
    _type_repr,
)

EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))
VERSION = sys.version_info[:2]
ROOT = pathlib.Path(__file__).resolve().parent.parent
FUZZ = settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=list(HealthCheck))


def shown(tp: typing.Any) -> str:
    return "None" if tp is None else repr(tp)


def types(found: dict[str, typing.Any]) -> str:
    return "; ".join(f"{name}={shown(tp)}" for name, tp in found.items())


def python_definitions(source: str, module: str | None) -> list[str]:
    return [
        "\n".join(
            [
                f"name: {name} {info.name}",
                f"kind: {info.kind}",
                "fields: " + "; ".join(f"{n}={t}" for n, t in info.fields),
                "required: " + ", ".join(sorted(info.required)),
                f"openness: {info.closed} {info.extra_items} {info.extra_items_readonly} "
                f"{shown(info.extra_items_type)} {info.owner}",
                "field_types: " + types(info.field_types),
                f"lineno: {info.lineno}",
                "bases: " + ", ".join(info.bases),
                "errors: " + "; ".join(info.errors),
                "columns: " + "; ".join(f"{c}={f}" for c, f in info.columns.items()),
                "opaque: " + ", ".join(sorted(info.opaque)),
                f"allows_extra: {info.allows_extra}",
                "lax: " + ", ".join(sorted(info.lax)),
                "annotations: "
                + "; ".join(f"{n}={ast.unparse(a)}" for n, a in info.annotations.items()),
                "coerced: "
                + ("None" if info.coerced is None else types(info.coerced().field_types)),
                f"module: {info.module}",
            ]
        )
        for name, info in extract_type_definitions(source, None, module).items()
    ]


COLUMNS = [
    None,
    "",
    "nope",
    "None",
    "list[int]",
    "int | str",
    "int | nope",
    "typing.Literal['a', 'b'] | None",
    "typing.Literal[1, 2, True]",
    "typing.Literal['a'] | str",
    "Literal['a']",
    "typing.Literal[1.5]",
    *PYTHON_TYPE_MAP,
]


def literals(values: typing.Iterable[str | int] | None, ordered: bool = False) -> str:
    if values is None:
        return "None"
    found = [(0, v) if isinstance(v, str) else (1, str(int(v))) for v in values]
    return (
        "["
        + ", ".join("si"[kind] + f":{v}" for kind, v in (found if ordered else sorted(found)))
        + "]"
    )


def python_relations(source: str, columns: list[str | None] = COLUMNS) -> list[str] | None:
    if (row := extract_type_definitions(source).get("Row")) is None:
        return None
    found = list(row.field_types.values())
    out = [
        "None"
        if tp is None
        else f"{_type_repr(tp)} {admits_none(tp)} {is_optional(tp)} {shown(_erase(tp))}"
        for tp in found
    ]
    for name, annotation in row.fields:
        tp = field_type(row, name)
        out += [
            f"{check_type_compatibility(row, name, column)} {shown(tp)} "
            f"{field_accepts(annotation, column)}"
            for column in columns
        ]
        out += [
            literals(None if tp is None else declared_literal_values(tp, [inferred]))
            for inferred in ("a", 1)
        ]
        items = parse_tuple_annotation(annotation)
        out.append("None" if items is None else "; ".join(f"{n}={t}" for n, t in items))
    for column in (c for c in columns if c is not None):
        out.append(literals(column_literal_values(column), ordered=True))
        out.append(str(check_type_compatibility(row, "missing", column)))
    out += [
        f"{_is_subtype(a, b)} {_is_subtype(a, b, _LAX_FROM)} {_consistent(a, b)}"
        for a, b in itertools.product(found, repeat=2)
    ]
    return out


def same(source: str, module: str | None = "m") -> list[str]:
    rust = type_definitions(source, module, VERSION)
    assert rust is not None, source
    assert rust == python_definitions(source, module), source
    return rust


def corpus() -> typing.Iterator[str]:
    for path in sorted((ROOT / "tests").rglob("*.py")):
        text = path.read_text()
        yield text
        for node in ast.walk(ast.parse(text)):
            if isinstance(node, ast.Constant) and isinstance(node.value, str):
                if re.search(r"class |TypedDict|NamedTuple", node.value):
                    yield node.value
    for folder in ("tests", "docs"):
        for path in sorted((ROOT / folder).rglob("*.md")):
            yield from re.findall(r"```(?:py|python)[^\n]*\n(.*?)```", path.read_text(), re.S)


def test_corpus() -> None:
    """The sources of the test suite, the modules its strings hold, and the docs' examples."""
    checked = found = 0
    for text in corpus():
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            try:
                ast.parse(text)
            except SyntaxError, ValueError:
                continue
        if type_definitions(text, "m", VERSION) is None:
            continue  # more than Rust reads: 256 KiB, a surrogate escape
        checked += 1
        found += len(same(text))
    assert checked > 1000 and found > 500


CASES = {
    "inconsistent bases": "class A(int, str): ...\nclass B(bool): ...\nclass C(object, int): ...\n"
    "class D(datetime.date, datetime.datetime): ...\nclass E(datetime.datetime, datetime.date): ...\n"
    "class F(E, uuid.UUID): ...\nclass G(ipaddress.IPv4Network, int): ...\nclass H(None): ...\n"
    "class I(list[int]): ...\nclass J(Any, int): ...\nclass K(K): ...\nclass L(int, int): ...\n",
    "cycles": "A = B\nB = A | int\nU = NewType('U', 'V')\nV = NewType('V', U | None)\n"
    "W = NewType('W', 'W')\nclass X(Y): ...\nclass Y(X): ...\n",
    "rebound": "A = int\nA = str\nB = int\ndef B(): ...\nC = int\nif x:\n    C = str\n"
    "D = int\ndef f():\n    D = str\nE = int\nfor E in y: ...\nF = int\nimport F\n",
    "strings": "A = 'int | None'\nB = ' list[\"A\"] '\nC = 'not valid('\nD = '''\nint'''\n",
    "literals": "A = Literal['a', 'a', 1, True, 0x10, 10**3]\nB = Literal[()]\nC = Literal[-1]\n"
    "D = Literal['a' 'b', None]\nE = Literal[1] | Literal[True] | Literal[1]\nF = Literal[1_0, 10]\n"
    "G = Literal[99999999999999999999999999, 'typing.x']\n",
    "generics": "A = tuple[()]\nB = tuple[int, ...]\nC = list[()]\nD = dict[str, Unknown]\n"
    "E = List[int]\nF = typing.Dict[str, t.Any]\nG = list[int][str]\nH = Optional[int, str]\n"
    "I = Union[int]\nJ = Union[int, int | None, None]\nK = Annotated[int | None, 1]\n",
    "stdlib": "A = pathlib.Path\nB = enum.StrEnum\nC = http.HTTPStatus\nD = collections.deque\n"
    "E = asynchat.async_chat\nF = builtins.ValueError\nG = typing.Any\nH = enum.Enum | Color\n"
    "class Color(enum.Enum): ...\nclass Level(enum.IntEnum): ...\nI = list[Level] | None\n",
}
HEAD = (
    "import typing, typing as t, datetime, uuid, ipaddress, pathlib, enum, http, collections\n"
    "import asynchat, builtins\n"
    "from typing import *\n"
    "from typing import Any, Optional, Union, List, Literal, Annotated, NewType, TypedDict\n"
)


@pytest.mark.parametrize("definitions", CASES.values(), ids=CASES.keys())
def test_cases(definitions: str) -> None:
    names = sorted(set(re.findall(r"^(?:class )?([A-Z])\b", definitions, re.M)))
    fields = "".join(f"    {name.lower()}: {name}\n" for name in names)
    for kind in ("TypedDict", "BaseModel"):
        source = f"{HEAD}{definitions}class Row({kind}):\n{fields}"
        assert len(same(source)) == 1
        assert type_relations(source, VERSION, COLUMNS) == python_relations(source), source


REAL = [
    "int",
    "str",
    "float",
    "bool",
    "bytes",
    "complex",
    "object",
    "list",
    "dict",
    "set",
    "frozenset",
    "tuple",
    "Any",
    "None",
    "datetime.date",
    "datetime.datetime",
    "datetime.time",
    "datetime.timedelta",
    "uuid.UUID",
    "decimal.Decimal",
    "ipaddress.IPv4Address",
    "ipaddress.IPv6Address",
    "ipaddress.IPv4Interface",
    "ipaddress.IPv6Interface",
    "ipaddress.IPv4Network",
    "ipaddress.IPv6Network",
]


@pytest.mark.parametrize("first", REAL)
def test_bases_python_can_combine(first: str) -> None:
    """Every class of two or three of the real classes: known where ``type()`` builds it,
    with the same subclasses."""
    head = "import datetime, uuid, decimal, ipaddress\nfrom typing import Any, TypedDict\n"
    pairs = [f"class P{i}({first}, {second}): ...\n" for i, second in enumerate(REAL)]
    triples = [
        f"class T{i}_{j}({first}, {second}, {third}): ...\n"
        for i, second in enumerate(REAL)
        for j, third in enumerate(REAL)
    ]
    derived = [f"class D{i}(P{i}, {second}): ...\n" for i, second in enumerate(REAL)]
    for classes, relations in ((pairs + derived, True), (triples, False)):
        names = re.findall(r"class (\w+)", "".join(classes))
        fields = "".join(f"    f{i}: {name}\n" for i, name in enumerate(names + REAL))
        source = f"{head}{''.join(classes)}class Row(TypedDict):\n{fields}"
        assert len(same(source)) == 1
        if relations:
            assert type_relations(source, VERSION, []) == python_relations(source, [])


def test_python_version() -> None:
    """A module is known in the Python versions that have it."""
    source = "import asynchat, annotationlib\nclass Row(TypedDict):\n    a: asynchat.async_chat\n    b: annotationlib.Format\n"
    old, new = (type_definitions(source, None, version) for version in ((3, 11), (3, 14)))
    assert old is not None and new is not None
    assert "a=<class 'asynchat.async_chat'>; b=None" in old[0]
    assert "a=None; b=<class 'annotationlib.Format'>" in new[0]


IMPORTS = [
    "import typing",
    "import typing as t",
    "import datetime",
    "import datetime as dt",
    "import decimal, uuid, ipaddress",
    "import pathlib, enum, http, collections",
    "import pydantic",
    "import pydantic.dataclasses",
    "import dataclasses",
    "from typing import Any, Optional, Union, List, Dict, Tuple, Literal, Annotated",
    "from typing import TypedDict, NamedTuple, NewType, Protocol, ClassVar, Never, TypeAlias",
    "from typing import Generic, TypeVar, NoReturn, Final",
    "from typing_extensions import ReadOnly, Required, NotRequired",
    "from typing import Optional as Opt, TypedDict as TD",
    "from decimal import Decimal",
    "from uuid import UUID",
    "from pathlib import Path",
    "from enum import Enum, StrEnum",
    "from collections import deque, OrderedDict",
    "from dataclasses import dataclass, field, KW_ONLY",
    "from pydantic import BaseModel, ConfigDict, Field, AliasChoices, Strict",
    "from pydantic import EmailStr, AnyUrl, SecretStr, ByteSize, Json, conint, StrictInt",
    "from pydantic import IPvAnyAddress, JsonValue, PositiveInt, Base64Bytes",
    "from pydantic.dataclasses import dataclass as pydantic_dataclass",
    "from other import Imported, UserId",
]
ATOMS = st.sampled_from(
    [
        "int",
        "str",
        "float",
        "bool",
        "bytes",
        "complex",
        "object",
        "None",
        "Any",
        "Never",
        "NoReturn",
        "list",
        "dict",
        "set",
        "frozenset",
        "tuple",
        "List",
        "Dict",
        "Tuple",
        "typing.Any",
        "t.List",
        "t.Any",
        "typing.int",
        "datetime.date",
        "datetime.datetime",
        "dt.date",
        "dt.time",
        "Decimal",
        "decimal.Decimal",
        "uuid.UUID",
        "UUID",
        "ipaddress.IPv4Address",
        "ipaddress.IPv6Network",
        "pathlib.Path",
        "Path",
        "enum.Enum",
        "Enum",
        "StrEnum",
        "http.HTTPStatus",
        "collections.OrderedDict",
        "deque",
        "pydantic.EmailStr",
        "EmailStr",
        "AnyUrl",
        "pydantic.AnyUrl",
        "SecretStr",
        "ByteSize",
        "pydantic.Json",
        "Json",
        "Json[int]",
        "conint(gt=1)",
        "pydantic.conint()",
        "StrictInt",
        "pydantic.StrictStr",
        "IPvAnyAddress",
        "JsonValue",
        "PositiveInt",
        "Base64Bytes",
        "UserId",
        "Tag",
        "Loop",
        "Email",
        "Color",
        "Both",
        "Sub",
        "Odd",
        "Plain",
        "A1",
        "A2",
        "A3",
        "Cyc1",
        "Cyc2",
        "Dup",
        "Imported",
        "Unknown",
        "mod.X",
        "T",
        "...",
        "1",
        "'int'",
        "'list[int]|None'",
        "'Email'",
        "Literal['a','b']",
        "Literal[1,True]",
        "Literal['a']",
        "Literal[1]",
        "tuple[()]",
        "typing.Literal[0]",
        "f(x)",
    ]
)


def _wrap(children: st.SearchStrategy[str]) -> st.SearchStrategy[str]:
    pair = st.tuples(children, children)
    return st.one_of(
        pair.map(" | ".join),
        children.map("Optional[{}]".format),
        children.map("Opt[{}]".format),
        children.map("t.Optional[{}]".format),
        pair.map(lambda p: f"Union[{p[0]}, {p[1]}]"),
        children.map("list[{}]".format),
        children.map("List[{}]".format),
        children.map("set[{}]".format),
        pair.map(lambda p: f"dict[{p[0]}, {p[1]}]"),
        pair.map(lambda p: f"tuple[{p[0]}, {p[1]}]"),
        children.map("tuple[{}, ...]".format),
        children.map("Annotated[{}, 1]".format),
        children.map("Annotated[{}, Field(default=1)]".format),
        children.map("Annotated[{}, Field(strict=True), Strict(False)]".format),
        children.map("Annotated[{}, Field(alias='al')]".format),
        children.map("ReadOnly[{}]".format),
        children.map("Required[{}]".format),
        children.map("NotRequired[{}]".format),
        children.map("ClassVar[{}]".format),
        children.map(repr),
        children.map("({})".format),
    )


ANNOTATIONS = st.recursive(ATOMS, _wrap, max_leaves=5)
DEFINITIONS = st.one_of(
    st.sampled_from(
        [
            "UserId = NewType('UserId', int)",
            "Loop = NewType('Loop', 'Loop')",
            "Cyc1 = Cyc2",
            "Cyc2 = Cyc1 | int",
            "class Email(str): ...",
            "class Color(enum.Enum): ...",
            "class Color(Enum):\n    RED = 1",
            "class Both(int, str): ...",
            "class Sub(Email, Plain): ...",
            "class Sub(Plain, Email): ...",
            "class Odd(datetime.datetime, datetime.date): ...",
            "class Odd(dt.date, Decimal): ...",
            "class Plain: ...",
            "@decorated\nclass Plain: ...",
            "class Plain(metaclass=M): ...",
            "class Plain:\n    def __get_validators__(cls): ...",
            "Dup = int",
            "Dup = str",
            "T = TypeVar('T')",
            "if flag:\n    A1 = str",
            "def UserId(): ...",
            "import Email",
        ]
    ),
    ANNOTATIONS.map("Tag = NewType('Tag', {})".format),
    ANNOTATIONS.map("A1 = {}".format),
    ANNOTATIONS.map("type A2 = {}".format),
    ANNOTATIONS.map("A3: TypeAlias = {}".format),
    ANNOTATIONS.map("A3: t.TypeAlias = {}".format),
)
VALUES = st.sampled_from(
    [
        "",
        " = 1",
        " = None",
        " = field(default=1)",
        " = field(default_factory=list)",
        " = field(init=False)",
        " = field()",
        " = Field(...)",
        " = Field(0)",
        " = Field(alias='al')",
        " = Field(validation_alias='va', alias='al')",
        " = Field(validation_alias=AliasChoices('a', 'b'))",
        " = Field(strict=True)",
        " = Field(strict=False, default=2)",
        " = pydantic.Field(alias=name)",
    ]
)
FIELD_NAMES = st.sampled_from(["a", "b", "c", "d", "_private", "_", "model_config"])
HEADERS = [
    "class {name}(TypedDict{base}):",
    "class {name}(TypedDict{base}, total=False):",
    "class {name}(TD{base}, closed=True):",
    "class {name}(TypedDict{base}, closed=False):",
    "class {name}(t.TypedDict{base}, extra_items={extra}):",
    "class {name}(TypedDict{base}, extra_items=ReadOnly[{extra}]):",
    "class {name}(TypedDict{base}, extra_items=Never):",
    "class {name}[T](TypedDict{base}):",
    "class {name}(TypedDict, Generic[T]{base}):",
    "class {name}(NamedTuple):",
    "class {name}(typing.NamedTuple):",
    "@dataclass\nclass {name}{bases}:",
    "@dataclasses.dataclass(frozen=True)\nclass {name}{bases}:",
    "@pydantic.dataclasses.dataclass\nclass {name}{bases}:",
    "@pydantic.dataclasses.dataclass(config=ConfigDict(strict=True))\nclass {name}{bases}:",
    "@pydantic_dataclass(config={{'strict': True, 'extra': 'allow'}})\nclass {name}{bases}:",
    "class {name}(BaseModel{base}):",
    "class {name}(pydantic.BaseModel{base}):",
    "class {name}(BaseModel{base}, strict=True):",
    "class {name}(BaseModel{base}, extra='forbid'):",
    "class {name}(BaseModel{base}, **options):",
    "class {name}(BaseModel, Generic[T]{base}):",
    "class {name}(Other{base}):",
    "class {name}(Protocol):",
    "class {name}(Protocol[T]):",
    "class {name}{bases}:",
]
CONFIGS = st.sampled_from(
    [
        "",
        "    model_config = ConfigDict(strict=True)\n",
        "    model_config = ConfigDict(populate_by_name=True, extra='allow')\n",
        "    model_config = {'extra': 'ignore', 'alias_generator': to_camel}\n",
        "    model_config: ConfigDict = ConfigDict(alias_generator=None, validate_by_name=True)\n",
        "    model_config = CONFIG\n",
        "    model_config = ConfigDict(**options)\n",
        "    __pydantic_config__ = ConfigDict(extra=Extra.forbid)\n",
        "    class Config:\n        strict = True\n        extra = 'allow'\n",
        "    x = y = 1\n",
        "    def method(self): ...\n",
    ]
)
CLASS_NAMES = ["Row", "Base", "Mid", "Leaf", "Other"]


@st.composite
def classes(draw: st.DrawFn) -> str:
    name = draw(st.sampled_from(CLASS_NAMES))
    parents = draw(st.lists(st.sampled_from(CLASS_NAMES), max_size=2))
    header = draw(st.sampled_from(HEADERS)).format(
        name=name,
        base="".join(f", {parent}" for parent in parents),
        bases=f"({', '.join(parents)})" if parents else "",
        extra=draw(ANNOTATIONS),
    )
    fields = draw(st.lists(st.tuples(FIELD_NAMES, ANNOTATIONS, VALUES), max_size=4))
    body = draw(CONFIGS) + "".join(f"    {n}: {a}{v}\n" for n, a, v in fields)
    text = f"{header}\n{body or '    pass\n'}"
    match draw(st.integers(0, 9)):
        case 0:
            return "def scope():\n" + "".join(f"    {line}\n" for line in text.splitlines())
        case 1:
            return "if flag:\n" + "".join(f"    {line}\n" for line in text.splitlines())
    return text


@st.composite
def functional(draw: st.DrawFn) -> str:
    name = draw(st.sampled_from(CLASS_NAMES))
    fields = draw(st.lists(st.tuples(st.sampled_from("abc"), ANNOTATIONS), max_size=3))
    match draw(st.integers(0, 3)):
        case 0:
            items = ", ".join(f"({n!r}, {a})" for n, a in fields)
            return f"{name} = NamedTuple({name!r}, [{items}])\n"
        case 1:
            items = ", ".join(f"{n!r}: {a}" for n, a in fields)
            return f"{name} = TypedDict({name!r}, {{{items}}}, total=False)\n"
        case 2:
            items = ", ".join(f"{n!r}: {a}" for n, a in fields)
            return f"{name} = t.TypedDict({name!r}, {{{items}, **more}}, closed=True)\n"
    items = ", ".join(f"{n!r}: {a}" for n, a in fields)
    return f"{name} = TypedDict({name!r}, {{{items}}}, extra_items={draw(ANNOTATIONS)})\n"


FAMILIES = {
    "TypedDict": [
        "class {name}(TypedDict{base}):",
        "class {name}(TypedDict{base}, total=False):",
        "class {name}(TypedDict{base}, closed=True):",
        "class {name}(TypedDict{base}, closed=False):",
        "class {name}(TypedDict{base}, extra_items={extra}):",
        "class {name}(TypedDict{base}, extra_items=ReadOnly[{extra}]):",
        "class {name}(TypedDict{base}, extra_items=Never):",
    ],
    "BaseModel": [
        "class {name}(BaseModel{base}):",
        "class {name}(BaseModel{base}, strict=True):",
        "class {name}(BaseModel{base}, extra='allow'):",
    ],
    "dataclass": [
        "@dataclass\nclass {name}{bases}:",
        "@pydantic.dataclasses.dataclass(config=ConfigDict(strict=True))\nclass {name}{bases}:",
    ],
}
SIMPLE = st.sampled_from(
    [
        "int",
        "str",
        "float",
        "bool",
        "object",
        "Any",
        "Never",
        "UserId",
        "Email",
        "Unknown",
        "int|None",
        "list[int]",
        "Literal['a']",
        "ReadOnly[int]",
        "NotRequired[str]",
        "Required[bool]",
        "Annotated[int,Field(default=1)]",
    ]
)


@st.composite
def families(draw: st.DrawFn) -> str:
    """Classes of one kind, each deriving from some of those before it."""
    headers = FAMILIES[draw(st.sampled_from(sorted(FAMILIES)))]
    out = []
    for i in range(draw(st.integers(2, 4))):
        parents = draw(st.lists(st.sampled_from([f"F{j}" for j in range(i)] or ["F0"]), max_size=2))
        header = draw(st.sampled_from(headers)).format(
            name=f"F{i}",
            base="".join(f", {parent}" for parent in parents),
            bases=f"({', '.join(parents)})" if parents else "",
            extra=draw(SIMPLE),
        )
        fields = draw(st.lists(st.tuples(st.sampled_from("abc"), SIMPLE, VALUES), max_size=3))
        body = draw(CONFIGS) + "".join(f"    {n}: {a}{v}\n" for n, a, v in fields)
        out.append(f"{header}\n{body or '    pass\n'}")
    return "".join(out)


@st.composite
def modules(draw: st.DrawFn) -> str:
    imports = draw(st.lists(st.sampled_from(IMPORTS), unique=True, min_size=len(IMPORTS) - 4))
    parts = draw(
        st.lists(
            st.one_of(DEFINITIONS.map("{}\n".format), classes(), functional(), families()),
            max_size=8,
        )
    )
    return "\n".join(imports) + "\n" + "".join(parts)


def parses(source: str) -> bool:
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            ast.parse(source)
        except SyntaxError:
            return False
    return True


@FUZZ
@given(modules(), st.sampled_from([None, "pkg.mod"]))
def test_generated_modules(source: str, module: str | None) -> None:
    if parses(source):
        same(source, module)


@FUZZ
@given(
    st.lists(st.sampled_from(IMPORTS), unique=True, min_size=len(IMPORTS) - 2),
    st.lists(DEFINITIONS, max_size=6),
    st.lists(ANNOTATIONS, min_size=1, max_size=6),
    st.sampled_from(["TypedDict", "BaseModel", "NamedTuple"]),
)
def test_generated_relations(
    imports: list[str], definitions: list[str], annotations: list[str], kind: str
) -> None:
    fields = "".join(f"    f{i}: {a}\n" for i, a in enumerate(annotations))
    source = "\n".join([*imports, *definitions]) + f"\nclass Row({kind}):\n{fields}"
    if parses(source):
        assert type_relations(source, VERSION, COLUMNS) == python_relations(source), source


# Assigned in every Unicode version the two sides may carry; newer characters may print
# escaped on one side only.
TEXT = st.text(
    st.sampled_from(
        "ab'\"\\\n\t\r {}%é\x00\x7f\x85\xa0\xad\u0378\u2028\u3000\ud7ff\ue000\U0001f600\U000e0001"
    ),
    max_size=6,
)
LEAVES = st.one_of(
    st.sampled_from(
        ["a", "b.c", "None", "True", "...", "1", "0x1f", "1_0", "1.5", "1e100", "1e-7"]
    ),
    st.sampled_from(["2j", "1.5e300j", "99999999999999999999", "0o17", "0b1", "1e999", ".5"]),
    TEXT.map(repr),
    TEXT.map(lambda text: "u" + repr(text)),
    TEXT.map(lambda text: repr(text.encode())),
    st.tuples(TEXT, TEXT).map(lambda p: f"{p[0]!r} {p[1]!r}"),
)


def _expressions(children: st.SearchStrategy[str]) -> st.SearchStrategy[str]:
    pair = st.tuples(children, children)
    binary = st.sampled_from(
        [
            "+",
            "-",
            "*",
            "/",
            "//",
            "%",
            "@",
            "**",
            "<<",
            ">>",
            "|",
            "^",
            "&",
            "and",
            "or",
            "==",
            "!=",
            "<",
            "<=",
            ">",
            ">=",
            "is",
            "in",
        ]
        + ["is not", "not in"]
    )
    unary = st.sampled_from(["-", "+", "~", "not ", "await ", "*"])
    return st.one_of(
        st.tuples(children, binary, children).map(" ".join),
        st.tuples(children, binary, children).map(lambda p: f"({p[0]}) {p[1]} ({p[2]})"),
        st.tuples(unary, children).map("".join),
        st.tuples(unary, children).map(lambda p: f"{p[0]}({p[1]})"),
        pair.map(lambda p: f"{p[0]} if {p[1]} else {p[0]}"),
        pair.map(lambda p: f"({p[0]} if {p[1]} else {p[0]}) if {p[1]} else {p[0]}"),
        pair.map(lambda p: f"lambda x, /, y={p[0]}, *z, k, **kw: {p[1]}"),
        children.map("lambda: {}".format),
        pair.map(lambda p: f"{p[0]}[{p[1]}]"),
        pair.map(lambda p: f"{p[0]}[{p[1]}, {p[1]}]"),
        pair.map(lambda p: f"{p[0]}[{p[1]}:, ::{p[0]}]"),
        pair.map(lambda p: f"{p[0]}({p[1]}, *{p[1]}, k={p[0]}, **{p[1]})"),
        children.map("({}).attr".format),
        children.map("({},)".format),
        pair.map(lambda p: f"({p[0]}, {p[1]})"),
        pair.map(lambda p: f"[{p[0]}, *{p[1]}]"),
        pair.map(lambda p: f"{{{p[0]}, {p[1]}}}"),
        pair.map(lambda p: f"{{{p[0]}: {p[1]}, **{p[0]}}}"),
        pair.map(lambda p: f"[{p[0]} for x in {p[1]} if x async for y, z in w]"),
        pair.map(lambda p: f"{{{p[0]}: {p[1]} for x in y}}"),
        pair.map(lambda p: f"f({p[0]} for x in {p[1]})"),
        pair.map(lambda p: f"(x := {p[0]}, {p[1]})"),
        children.map("(yield {})".format),
        children.map("(yield from {})".format),
        pair.map(lambda p: f"f'{{{p[0]}!r:>{{{p[1]}}}}} {{x=}} {{{{'"),
        pair.map(lambda p: f'f"""{{{p[0]}}}\n{{ {p[1]} = :x}}"""'),
        pair.map(lambda p: f"t'{{{p[0]}!s}} {{{p[1]}:{{w}}.2f}}'"),
        children.map("f'{{ {} }}' 'tail'".format),
    )


EXPRESSIONS = st.recursive(LEAVES, _expressions, max_leaves=6)


@FUZZ
@given(EXPRESSIONS)
def test_generated_expressions(source: str) -> None:
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            expected = ast.unparse(ast.parse(source, mode="eval").body)
        except SyntaxError, ValueError:
            return
    assert unparse_expression(source) == expected, source
    assert unparse_expression(expected) == expected, source


def test_expressions_of_the_package() -> None:
    """Every expression of sqlty's own source, as written and as unparsed."""
    checked = 0
    for path in sorted((ROOT / "src" / "sqlty").rglob("*.py")):
        if path.name == "_stdlib.py":
            continue
        text = path.read_text()
        for node in ast.walk(ast.parse(text)):
            if isinstance(node, ast.expr) and not isinstance(node, (ast.Slice, ast.Starred)):
                expected = ast.unparse(node)
                written = ast.get_source_segment(text, node)
                assert written is not None
                for source in (f"({written}\n)", expected):
                    try:  # the text of an f-string is not an expression
                        if ast.unparse(ast.parse(source, mode="eval").body) != expected:
                            continue
                    except SyntaxError:
                        continue
                    if (found := unparse_expression(source)) is not None:
                        assert found == expected, source
                        checked += 1
    assert checked > 50_000


def expected_unparse(source: str) -> str | None:
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        try:
            return ast.unparse(ast.parse(source, mode="eval").body)
        except SyntaxError, ValueError:
            return None


SPELLINGS = [
    "'''a\r\nb'''",
    "b'''a\rb'''",
    "r'''a\r\nb'''",
    "f'''{x:a\rb}'''",
    "t'''{a +\r\n b}'''",
    "t'''{a +\r b}'''",
    "f'''{x # c\n=}'''",
    "t'''{a # c\n}'''",
    "t'''{a +# c\n b}'''",
    "t'''{a + '#' # c\n}'''",
    "t'{x!=1:y}'",
    "t'{a!=b!=c:y}'",
    "t'{a if b!=c else d:>3}'",
    "f'{x!=1=:y}'",
    "t'{x!=1}'",
    "t'{x!=1!r}'",
    "t'{a[b!=c]:y}'",
    "t'{\"!\":y}'",
    "f'{x = !r}'",
    "t'{ x = }'",
    "967742817868973.25",
    "2126528249093299.25j",
    "9007199254740993.0",
    "5e-324",
    "1e16",
    "9" * 4300,
    "9" * 4301,
]


@pytest.mark.parametrize("source", SPELLINGS)
def test_spellings(source: str) -> None:
    assert unparse_expression(source) == expected_unparse(source)


@FUZZ
@given(st.floats(allow_nan=False, allow_infinity=False, min_value=0), st.booleans())
def test_generated_floats(value: float, imaginary: bool) -> None:
    source = repr(value) + "j" * imaginary
    assert unparse_expression(source) == expected_unparse(source), source


FIELD_PARTS = st.sampled_from(
    [
        "x",
        "a + b",
        "a != b",
        "a!=b",
        "d['k']",
        'd["!"]',
        "f(a, b)",
        "(a\n + b)",
        "a # c\n",
        "a +# '\n b",
    ]
    + [
        " x ",
        "x=",
        "x = ",
        "x!r",
        "x!s",
        "x:>3",
        "x!r:>3",
        "x=:y",
        "x=!a",
        "x:{w}",
        "x\r\n",
        "x:a\rb",
    ]
)


@FUZZ
@given(
    st.sampled_from(["f", "t", "F", "rf", "tr"]),
    st.sampled_from(["'''", '"""']),
    st.lists(
        st.one_of(
            st.sampled_from(["a", " ", "\r\n", "\r", "{{", "#", "'"]),
            FIELD_PARTS.map("{{{}}}".format),
        )
    ),
)
def test_generated_interpolations(prefix: str, quote: str, parts: list[str]) -> None:
    source = f"{prefix}{quote}{''.join(parts)}{quote}"
    if (expected := expected_unparse(source)) is not None:
        assert unparse_expression(source) == expected, source


LINES = {
    "decorated class on a continued line": (
        "from dataclasses import dataclass\n@dataclass\nclass \\\n    Row:\n    a: int\n"
    ),
    "typing class": "import typing\nclass Row(typing.TypedDict):\n    a: typing.Pattern\n",
    "CRLF": "from typing import Literal, TypedDict\r\nclass Row(TypedDict):\r\n    a: Literal['''x\r\ny''']\r\n",
    "under try": "from typing import TypedDict\ntry:\n    class Row(TypedDict):\n        a: int\nexcept E:\n    class Row(TypedDict):\n        b: int\n",
    "nested": "from typing import TypedDict\nclass Outer:\n    class Row(TypedDict):\n        a: int\n",
}


@pytest.mark.parametrize("source", LINES.values(), ids=LINES.keys())
def test_lines(source: str) -> None:
    same(source)


def test_surrogate_in_a_string_annotation_is_declined() -> None:
    source = "from typing import Literal, TypedDict\nclass Row(TypedDict):\n    a: \"Literal['\\\\ud800']\"\n"
    assert type_definitions(source, None, VERSION) is None
