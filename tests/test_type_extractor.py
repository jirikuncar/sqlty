"""Unit tests for _type_extractor module."""

import ast
import contextlib
import datetime
import sys
import typing
import uuid
from collections.abc import Sequence
from pathlib import Path
from unittest import mock

import pytest

from sqlty._type_extractor import (
    TypeInfo,
    _consistent,
    _erase,
    _is_subtype,
    _newtype_assignments,
    check_type_compatibility,
    extract_type_definitions,
    field_accepts,
    parse_tuple_annotation,
)
from sqlty.core import check_source


def extract(source: str, *, runtime: bool) -> dict[str, TypeInfo]:
    """Extract types, without runtime resolution (the AST fallback) when ``runtime`` is false."""
    with (
        contextlib.nullcontext()
        if runtime
        else mock.patch("sqlty._type_extractor._resolve_class", return_value=None)
    ):
        return extract_type_definitions(source)


class TestParseTupleAnnotation:
    """Tests for parse_tuple_annotation."""

    def test_single_type(self):
        result = parse_tuple_annotation("tuple[int]")
        assert result == [("_0", "int")]

    def test_two_types(self):
        result = parse_tuple_annotation("tuple[int, str]")
        assert result == [("_0", "int"), ("_1", "str")]

    def test_three_types(self):
        result = parse_tuple_annotation("tuple[int, str, bool]")
        assert result == [("_0", "int"), ("_1", "str"), ("_2", "bool")]

    def test_complex_types(self):
        result = parse_tuple_annotation("tuple[int, datetime.datetime, uuid.UUID]")
        assert result == [
            ("_0", "int"),
            ("_1", "datetime.datetime"),
            ("_2", "uuid.UUID"),
        ]

    def test_nested_generic(self):
        result = parse_tuple_annotation("tuple[list[int], dict[str, int]]")
        assert result == [("_0", "list[int]"), ("_1", "dict[str, int]")]

    def test_not_tuple(self):
        assert parse_tuple_annotation("User") is None
        assert parse_tuple_annotation("int") is None
        assert parse_tuple_annotation("list[int]") is None

    def test_empty_tuple(self):
        result = parse_tuple_annotation("tuple[]")
        assert result == []


class TestExtractTypeDefinitions:
    """Tests for extract_type_definitions."""

    def test_typeddict(self):
        source = """
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str
"""
        types = extract_type_definitions(source)
        assert "User" in types
        user = types["User"]
        assert user.kind == "TypedDict"
        assert user.fields == [("id", "int"), ("name", "str")]

    def test_namedtuple(self):
        source = """
from typing import NamedTuple

class UserRow(NamedTuple):
    id: int
    name: str
    email: str
"""
        types = extract_type_definitions(source)
        assert "UserRow" in types
        user = types["UserRow"]
        assert user.kind == "NamedTuple"
        assert user.fields == [("id", "int"), ("name", "str"), ("email", "str")]

    def test_dataclass(self):
        source = """
from dataclasses import dataclass

@dataclass
class Product:
    id: int
    name: str
    price: float
"""
        types = extract_type_definitions(source)
        assert "Product" in types
        product = types["Product"]
        assert product.kind == "dataclass"
        assert product.fields == [("id", "int"), ("name", "str"), ("price", "float")]

    def test_pydantic_basemodel(self):
        source = """
from pydantic import BaseModel

class UserModel(BaseModel):
    id: int
    name: str
    email: str
"""
        types = extract_type_definitions(source)
        assert "UserModel" in types
        user = types["UserModel"]
        assert user.kind == "BaseModel"
        assert user.fields == [("id", "int"), ("name", "str"), ("email", "str")]

    def test_multiple_types(self):
        source = """
from typing import TypedDict, NamedTuple

class User(TypedDict):
    id: int
    name: str

class EventRow(NamedTuple):
    event_id: int
    created_at: str
"""
        types = extract_type_definitions(source)
        assert len(types) == 2
        assert "User" in types
        assert "EventRow" in types

    def test_datetime_types(self):
        source = """
import datetime
from typing import TypedDict

class Event(TypedDict):
    id: int
    created_at: datetime.datetime
    event_date: datetime.date
"""
        types = extract_type_definitions(source)
        assert "Event" in types
        event = types["Event"]
        assert event.fields == [
            ("id", "int"),
            ("created_at", "datetime.datetime"),
            ("event_date", "datetime.date"),
        ]

    def test_uuid_type(self):
        source = """
import uuid
from typing import TypedDict

class Token(TypedDict):
    token_id: uuid.UUID
    value: str
"""
        types = extract_type_definitions(source)
        assert "Token" in types
        token = types["Token"]
        assert token.fields == [("token_id", "uuid.UUID"), ("value", "str")]

    def test_optional_types_normalized(self):
        source = """
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str | None
    email: None | str
"""
        types = extract_type_definitions(source)
        assert "User" in types
        user = types["User"]
        assert user.fields == [
            ("id", "int"),
            ("name", "str"),
            ("email", "str"),
        ]

    def test_regular_class_ignored(self):
        source = """
class Foo:
    x: int
    y: str
"""
        types = extract_type_definitions(source)
        assert len(types) == 0

    def test_class_without_annotations_ignored(self):
        source = """
from typing import TypedDict

class Empty(TypedDict):
    pass
"""
        types = extract_type_definitions(source)
        assert len(types) == 0

    def test_invalid_source(self):
        types = extract_type_definitions("this is not valid python {{{")
        assert types == {}

    def test_dataclass_with_defaults(self):
        source = """
from dataclasses import dataclass

@dataclass
class Config:
    host: str
    port: int
    debug: bool
"""
        types = extract_type_definitions(source)
        assert "Config" in types
        config = types["Config"]
        assert config.fields == [
            ("host", "str"),
            ("port", "int"),
            ("debug", "bool"),
        ]

    def test_dataclass_with_call_decorator(self):
        """@dataclass() with parentheses should also be recognized."""
        source = """
from dataclasses import dataclass

@dataclass()
class Item:
    id: int
    name: str
"""
        types = extract_type_definitions(source)
        assert "Item" in types
        assert types["Item"].kind == "dataclass"

    def test_dataclass_frozen(self):
        """@dataclass(frozen=True) should be recognized."""
        source = """
from dataclasses import dataclass

@dataclass(frozen=True)
class Item:
    id: int
    name: str
"""
        types = extract_type_definitions(source)
        assert "Item" in types
        assert types["Item"].kind == "dataclass"


class TestRuntimeResolution:
    """Tests for runtime type resolution via get_type_hints()."""

    def test_resolved_hints_available(self):
        """TypedDict should have resolved_hints with actual type objects."""
        source = """
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str
"""
        types = extract_type_definitions(source)
        user = types["User"]
        assert user.resolved_hints is not None
        assert user.resolved_hints["id"] is int
        assert user.resolved_hints["name"] is str

    def test_datetime_resolved(self):
        """datetime.datetime annotation resolves to actual datetime class."""
        source = """
import datetime
from typing import TypedDict

class Event(TypedDict):
    id: int
    created_at: datetime.datetime
    event_date: datetime.date
"""
        types = extract_type_definitions(source)
        event = types["Event"]
        assert event.resolved_hints is not None
        assert event.resolved_hints["created_at"] is datetime.datetime
        assert event.resolved_hints["event_date"] is datetime.date

    def test_uuid_resolved(self):
        """uuid.UUID annotation resolves to actual UUID class."""
        source = """
import uuid
from typing import TypedDict

class Token(TypedDict):
    token_id: uuid.UUID
"""
        types = extract_type_definitions(source)
        token = types["Token"]
        assert token.resolved_hints is not None
        assert token.resolved_hints["token_id"] is uuid.UUID

    def test_optional_unwrapped(self):
        """Optional[int] and int | None resolve to int (unwrapped)."""
        source = """
from typing import TypedDict

class User(TypedDict):
    id: int | None
    name: str
"""
        types = extract_type_definitions(source)
        user = types["User"]
        assert user.resolved_hints is not None
        assert user.resolved_hints["id"] is int
        assert user.resolved_hints["name"] is str

    def test_from_datetime_import(self):
        """'from datetime import datetime' resolves correctly.

        This is a key advantage over AST string comparison where
        'datetime' != 'datetime.datetime'.
        """
        source = """
from datetime import datetime
from typing import TypedDict

class Event(TypedDict):
    id: int
    created_at: datetime
"""
        types = extract_type_definitions(source)
        event = types["Event"]
        assert event.resolved_hints is not None
        # Runtime resolution gives us the actual datetime class
        import datetime as dt

        assert event.resolved_hints["created_at"] is dt.datetime

    def test_dataclass_resolved(self):
        """Dataclass fields are resolved at runtime."""
        source = """
from dataclasses import dataclass

@dataclass
class Product:
    id: int
    name: str
    price: float
"""
        types = extract_type_definitions(source)
        product = types["Product"]
        assert product.resolved_hints is not None
        assert product.resolved_hints["id"] is int
        assert product.resolved_hints["name"] is str
        assert product.resolved_hints["price"] is float

    def test_namedtuple_resolved(self):
        """NamedTuple fields are resolved at runtime."""
        source = """
from typing import NamedTuple

class Row(NamedTuple):
    id: int
    name: str
"""
        types = extract_type_definitions(source)
        row = types["Row"]
        assert row.resolved_hints is not None
        assert row.resolved_hints["id"] is int
        assert row.resolved_hints["name"] is str


class TestCheckTypeCompatibility:
    """Tests for check_type_compatibility using runtime types."""

    def _make_type_info(self, source: str, name: str) -> TypeInfo:
        types = extract_type_definitions(source)
        return types[name]

    def test_exact_match(self):
        source = """
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str
"""
        user = self._make_type_info(source, "User")
        assert check_type_compatibility(user, "id", "int") is True
        assert check_type_compatibility(user, "name", "str") is True

    def test_type_mismatch(self):
        source = """
from typing import TypedDict

class User(TypedDict):
    id: str
    name: str
"""
        user = self._make_type_info(source, "User")
        assert check_type_compatibility(user, "id", "int") is False

    def test_optional_compatible(self):
        """int | None should be compatible with int."""
        source = """
from typing import TypedDict

class User(TypedDict):
    id: int | None
    name: str
"""
        user = self._make_type_info(source, "User")
        assert check_type_compatibility(user, "id", "int") is True

    def test_datetime_via_from_import(self):
        """'from datetime import datetime' should match 'datetime.datetime'.

        This is the key case where runtime checking beats string comparison.
        """
        source = """
from datetime import datetime
from typing import TypedDict

class Event(TypedDict):
    created_at: datetime
"""
        event = self._make_type_info(source, "Event")
        assert check_type_compatibility(event, "created_at", "datetime.datetime") is True

    def test_uuid_compatibility(self):
        source = """
import uuid
from typing import TypedDict

class Token(TypedDict):
    token_id: uuid.UUID
"""
        token = self._make_type_info(source, "Token")
        assert check_type_compatibility(token, "token_id", "uuid.UUID") is True

    def test_unknown_column_type(self):
        """None column type should always be compatible (can't validate)."""
        source = """
from typing import TypedDict

class User(TypedDict):
    id: int
"""
        user = self._make_type_info(source, "User")
        assert check_type_compatibility(user, "id", None) is True

    def test_missing_field(self):
        """Field not in the type should return False."""
        source = """
from typing import TypedDict

class User(TypedDict):
    id: int
"""
        user = self._make_type_info(source, "User")
        assert check_type_compatibility(user, "nonexistent", "int") is False


class TestTypedDictQualifiers:
    """Qualifiers and PEP 728 metadata, via runtime and AST fallback."""

    SOURCE = """\
{imports}
from typing import Never, NotRequired, ReadOnly, TypedDict

class Row(TypedDict, closed=True):
    id: ReadOnly[int]
    note: NotRequired[ReadOnly[str]]

class Extra(TypedDict, extra_items=ReadOnly[int]):
    day: str

class Sealed(TypedDict, extra_items=ReadOnly[Never]):
    id: int

class Child(Row):
    note: ReadOnly[str]

class GrandChild(Child):
    pass

class ExtraChild(Extra):
    pass

class Reopened(Extra, closed=False):
    pass
"""

    def _extract(self, runtime: bool) -> dict[str, TypeInfo]:
        return extract(self.SOURCE.format(imports=""), runtime=runtime)

    @pytest.mark.parametrize("runtime", [True, False])
    def test_runtime_and_ast_agree(self, runtime: bool):
        types = self._extract(runtime)
        row, extra = types["Row"], types["Extra"]
        assert (row.resolved_hints is not None) is runtime
        assert row.fields == [("id", "int"), ("note", "str")]
        assert row.required == {"id"}
        assert row.closed and row.keyed and row.extra_items is None
        assert extra.extra_items == "int" and not extra.closed
        assert check_type_compatibility(extra, "views", "int")
        assert not check_type_compatibility(extra, "ratio", "float")

    def test_extra_items_never_is_closed(self):
        for runtime in (True, False):
            sealed = self._extract(runtime)["Sealed"]
            assert sealed.closed and sealed.extra_items is None
            assert sealed.extra_items_hint is None

    def test_subclasses_inherit_keys_and_closedness(self):
        for runtime in (True, False):
            types = self._extract(runtime)
            for name in ("Child", "GrandChild"):
                child = types[name]
                assert child.fields == [("id", "int"), ("note", "str")]
                assert child.required == {"id", "note"}, name
                assert child.closed and child.extra_items is None
            extra_child = types["ExtraChild"]
            assert extra_child.extra_items == "int" and not extra_child.closed
            assert not check_type_compatibility(extra_child, "ratio", "float")

    def test_closed_false_cannot_reopen_base(self):
        for runtime in (True, False):
            reopened = self._extract(runtime)["Reopened"]
            assert reopened.extra_items == "int" and not reopened.closed
            assert reopened.errors == [
                "Reopened cannot set closed=False: base Extra has extra_items"
            ]


INHERITANCE_SOURCE = """\
{imports}
from typing import NotRequired, ReadOnly, TypedDict

class Closed(TypedDict, closed=True):
    id: int

class AddsKey(Closed):
    name: str

class Widens(Closed, extra_items=str):
    id: int

class Mutable(TypedDict, extra_items=int | None):
    id: int

class Seals(Mutable, closed=True):
    id: int

class Retypes(Mutable, extra_items=str):
    id: int

class RequiredKey(Mutable):
    views: int | None

class WrongKey(Mutable):
    views: NotRequired[str]

class Scores(TypedDict, extra_items=ReadOnly[int]):
    id: int

class Floats(Scores):
    score: float

class Narrows(Scores, extra_items=bool):
    id: int
"""


class TestTypedDictInheritance:
    """PEP 728 inheritance errors and openness fallback, via runtime and AST."""

    CASES: typing.ClassVar = {
        "AddsKey": ["AddsKey cannot add key 'name' to closed base Closed"],
        "Widens": ["Widens cannot set extra_items=str: base Closed is closed"],
        "Seals": ["Seals cannot set closed=True: base Mutable has mutable extra_items int | None"],
        "Retypes": [
            "Retypes cannot set extra_items=str: base Mutable has mutable extra_items int | None"
        ],
        "RequiredKey": [
            "RequiredKey.views must be NotRequired: base Mutable has extra_items int | None"
        ],
        "WrongKey": ["WrongKey.views is str but base Mutable has extra_items int | None"],
        "Floats": ["Floats.score is float but base Scores has read-only extra_items int"],
        "Narrows": [],
    }

    @pytest.mark.parametrize("runtime", [True, False])
    @pytest.mark.parametrize("name", list(CASES))
    def test_errors(self, name: str, runtime: bool):
        types = extract(INHERITANCE_SOURCE.format(imports=""), runtime=runtime)
        assert (types[name].resolved_hints is not None) is runtime
        assert types[name].errors == self.CASES[name]

    @pytest.mark.parametrize("runtime", [True, False])
    def test_rejected_override_keeps_base_openness(self, runtime: bool):
        types = extract(INHERITANCE_SOURCE.format(imports=""), runtime=runtime)
        widens, seals, retypes = types["Widens"], types["Seals"], types["Retypes"]
        assert widens.closed and widens.extra_items is None and widens.owner == "Closed"
        for info in (seals, retypes):
            assert not info.closed and info.extra_items == "int" and not info.extra_items_readonly
            assert info.extra_items_type == int | None and info.owner == "Mutable"
        narrows = types["Narrows"]
        assert narrows.extra_items == "bool" and not narrows.extra_items_readonly
        assert narrows.extra_items_type is bool and narrows.owner == "Narrows"


@pytest.mark.parametrize(
    ("src", "dst", "subtype", "consistent"),
    [
        (int, int, True, True),
        (bool, int, True, False),
        (int, bool, False, False),
        (int, float, True, False),
        (float, complex, True, False),
        (float, int, False, False),
        (int | None, None | int, True, True),
        (int, int | None, True, False),
        (int | None, int, False, False),
        (typing.Any, int, True, True),
        (int, typing.Any, True, True),
        (int, object, True, False),
        (int | typing.Any, int, True, True),
        (int, int | typing.Any, True, True),
        (float, int | typing.Any, True, True),
        (int | typing.Any, object, True, True),
        (str | typing.Any, int, False, False),
        (typing.Never, int, True, False),
        (list[int], list, None, None),
        (list[int], Sequence[int], None, None),
        (typing.Literal["x"], str, None, None),
        (str, None, None, None),
        (int, typing.NewType("UserId", int), None, None),
        (list, list[int], None, None),
    ],
)
def test_subtype_and_consistency(src, dst, subtype, consistent):
    assert _is_subtype(src, dst) is subtype
    assert _consistent(src, dst) is consistent


@pytest.mark.parametrize(
    ("annotation", "column", "ok"),
    [
        ("Any", "int", True),
        ("int | Any", "int", True),
        ("Optional[Union[str, Any]]", "int", True),
        ("int | str", "int", True),
        ("float", "int", True),
        ("str | bytes", "int", False),
        ("bool", "int", False),
        ("str", None, True),
        ("list[str]", "list", True),
        ("list[list[int]] | None", "list", True),
        ("dict[str, int]", "list", False),
        ("typing.List[str]", "list", True),
        ("List[str]", "list", True),
        ("typing.Dict[str, int]", "list", False),
    ],
)
def test_field_accepts(annotation: str, column: str | None, ok: bool):
    """String-only annotations (``SQL[tuple[...]]`` items) use the same subtype check."""
    assert field_accepts(annotation, column) is ok


UserId = typing.NewType("UserId", int)


@pytest.mark.parametrize(
    ("tp", "erased"),
    [
        (UserId, int),
        (typing.NewType("AdminId", UserId), int),
        (typing.NewType("Tags", list[str]), list),
        (dict[str, list[int]], dict),
        (UserId | list[str], int | list),
        (typing.Literal["x"], typing.Literal["x"]),
        (None, None),
    ],
)
def test_erase(tp, erased):
    assert _erase(tp) == erased


@pytest.mark.parametrize(
    ("imports", "annotation", "expected"),
    [
        ("import typing as t", "t.List[str]", list[str]),
        ("import typing", "typing.List[str]", list[str]),
        ("from typing import List", "List[str]", list[str]),
        ("from typing import List as L", "L[str]", list[str]),
        ("from typing import Dict", "Dict[str, int]", dict[str, int]),
        ("from typing import Set", "Set[int]", set[int]),
        ("from typing import FrozenSet", "FrozenSet[int]", frozenset[int]),
        ("from typing import Tuple", "Tuple[int, str]", tuple[int, str]),
        ("import typing_extensions", "typing_extensions.List[str]", list[str]),
        ("from typing_extensions import List", "List[str]", list[str]),
        ("from mylib import List", "List[str]", None),
        ("class List: ...", "List[str]", None),
    ],
)
def test_typing_generics_without_runtime_resolution(imports, annotation, expected):
    src = f"import not_installed\n{imports}\nfrom typing import TypedDict\nclass Row(TypedDict):\n    x: {annotation}\n"
    row = extract_type_definitions(src)["Row"]
    assert row.field_types["x"] == expected


def test_newtype_assignments_only_type_expressions():
    src = """
A = NewType("A", int)
B = NewType("B", list[str] | None)
C = NewType("Other", int)
D = NewType("D", print("x") or int)
E = NewType("E", f())
F = NewType("F", int + str)
"""
    assert list(_newtype_assignments(ast.parse(src), {})) == ["A", "B"]


def test_fields_resolve_one_by_one():
    src = """\
from __future__ import annotations
from datetime import datetime
from typing import TypedDict
from myapp.ids import UserId
import not_installed.sub

UserName = str
type Born = datetime

class Row(TypedDict):
    id: UserId
    name: UserName | None
    created_at: datetime
    born: Born
    other: not_installed.sub.Thing
    missing: Undefined
"""
    row = extract_type_definitions(src)["Row"]
    assert row.resolved_hints == {
        "name": str,
        "created_at": datetime.datetime,
        "born": datetime.datetime,
    }
    assert check_type_compatibility(row, "id", "int")
    assert check_type_compatibility(row, "missing", "int")
    assert not check_type_compatibility(row, "name", "int")


MARKER_MODULE = "import pathlib\npathlib.Path({marker!r}).write_text('imported')\nfrom typing import NewType\nUserId = NewType('UserId', int)\n"
MARKER_SOURCE = """
from typing import TypedDict
from marker_app_ids import UserId
from sqlty import SQL

class User(TypedDict):
    id: UserId

q = SQL[User]("--sql\\nSELECT id::TEXT FROM users")
"""


@pytest.fixture
def marker_module(tmp_path, monkeypatch) -> Path:
    marker = tmp_path / "marker"
    (tmp_path / "marker_app_ids.py").write_text(MARKER_MODULE.format(marker=str(marker)))
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.delitem(sys.modules, "marker_app_ids", raising=False)
    return marker


def test_application_imports_are_not_executed(marker_module) -> None:
    assert check_source(MARKER_SOURCE) == []
    assert not marker_module.exists()


def test_type_modules_allow_imports(marker_module) -> None:
    diagnostics = check_source(MARKER_SOURCE, type_modules=["marker_app_ids"])
    assert marker_module.exists()
    assert [d["rule"] for d in diagnostics] == ["column-type-mismatch"]
