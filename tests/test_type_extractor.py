"""Unit tests for _type_extractor module."""

import datetime
import uuid

from sqlty._type_extractor import (
    TypeInfo,
    check_type_compatibility,
    extract_type_definitions,
    parse_tuple_annotation,
)


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
