"""Row-type description (``TypeInfo``) and the SQL-to-Python type map."""

from __future__ import annotations

import ast
import datetime
import decimal
import functools
import ipaddress
import operator
import typing
import uuid

if typing.TYPE_CHECKING:
    pass


class TypeInfo:
    """Information about a user-defined type extracted from source.

    ``required``, ``closed`` and ``extra_items`` describe TypedDict keys
    (PEP 655, PEP 728); other kinds have every field required and are open.
    For TypedDicts, ``extra_items_readonly`` and ``extra_items_type`` (the
    resolved type, or None when unknown) complete ``extra_items``. ``field_types``
    holds each field's type resolved from source (None when unknown), ``owner``
    names the class whose header set the openness, ``lineno`` is the class
    line, ``bases`` names its TypedDict bases defined in the same source and
    ``errors`` lists its PEP 728 inheritance violations. For Pydantic models,
    ``columns`` maps each accepted column name (field name or validation alias)
    to its field, ``opaque`` names the fields whose validation names are not
    statically known (``AliasChoices``, ``alias_generator``) and ``allows_extra``
    whether it keeps undeclared columns (``extra="allow"``, or a config not read from source). ``lax`` names the fields of a Pydantic model or
    dataclass that accept lax-mode conversions (not strict). ``annotations`` holds
    each field's annotation from source, qualifiers stripped and ``| None`` kept, and
    ``aliases`` the typing names of the source it was read from (see
    ``ParsedSource.typing_aliases``); JSON shapes are checked against them.
    """

    __slots__ = (
        "name",
        "fields",
        "kind",
        "resolved_hints",
        "declared_hints",
        "required",
        "closed",
        "extra_items",
        "extra_items_hint",
        "extra_items_readonly",
        "extra_items_type",
        "field_types",
        "owner",
        "lineno",
        "bases",
        "errors",
        "columns",
        "opaque",
        "allows_extra",
        "lax",
        "annotations",
        "aliases",
    )

    def __init__(
        self,
        name: str,
        fields: list[tuple[str, str]],
        kind: str,
        resolved_hints: dict[str, type] | None = None,
        *,
        required: frozenset[str] | None = None,
        closed: bool = False,
        extra_items: str | None = None,
        extra_items_hint: type | None = None,
    ) -> None:
        self.name = name
        self.fields = fields  # [(field_name, field_type_str), ...] — AST fallback
        self.kind = kind  # "TypedDict", "NamedTuple", "dataclass", "BaseModel"
        self.resolved_hints = resolved_hints  # {field_name: type_object} — runtime
        self.required = frozenset(n for n, _ in fields) if required is None else required
        self.closed = closed
        self.extra_items = extra_items
        self.extra_items_hint = extra_items_hint
        self.extra_items_readonly = False
        self.extra_items_type: typing.Any = None
        self.field_types: dict[str, typing.Any] = {}
        self.declared_hints: dict[str, typing.Any] = {}  # runtime hints, ``| None`` kept
        self.owner = name
        self.lineno = 0
        self.bases: list[str] = []
        self.errors: list[str] = []
        self.columns: dict[str, str] = {}
        self.opaque: frozenset[str] = frozenset()
        self.allows_extra = False
        self.lax: frozenset[str] = frozenset()
        self.annotations: dict[str, ast.expr] = {}
        self.aliases: dict[str, str] = {}

    def field_for(self, column: str) -> str | None:
        """The field a query column fills, by name (a Pydantic alias counts); None if none does."""
        if self.kind == "BaseModel":
            return self.columns.get(column)
        return column if column in dict(self.fields) else None

    def column_names(self, field: str) -> list[str]:
        """The query column names that fill ``field``, the Pydantic alias first."""
        if self.kind == "BaseModel":
            return [c for c, f in self.columns.items() if f == field]
        return [field]

    @property
    def keyed(self) -> bool:
        """Whether columns match keys by name (closed or ``extra_items`` TypedDict)."""
        return self.closed or self.extra_items is not None

    def __repr__(self) -> str:
        return f"TypeInfo({self.name!r}, fields={self.fields!r}, kind={self.kind!r})"


PYTHON_TYPE_MAP: dict[str, type] = {
    "int": int,
    "str": str,
    "float": float,
    "bool": bool,
    "list": list,
    "dict": dict,
    "datetime.date": datetime.date,
    "datetime.datetime": datetime.datetime,
    "datetime.time": datetime.time,
    "datetime.timedelta": datetime.timedelta,
    "uuid.UUID": uuid.UUID,
    "decimal.Decimal": decimal.Decimal,
    "typing.Any": typing.Any,
    "None": type(None),
    "ipaddress.IPv4Address": ipaddress.IPv4Address,
    "ipaddress.IPv6Address": ipaddress.IPv6Address,
    "ipaddress.IPv4Interface": ipaddress.IPv4Interface,
    "ipaddress.IPv6Interface": ipaddress.IPv6Interface,
    "ipaddress.IPv4Network": ipaddress.IPv4Network,
    "ipaddress.IPv6Network": ipaddress.IPv6Network,
}


def _column_type(python_type: str) -> typing.Any:
    """The type object of an inferred column type; ``list[T]`` is ``list``, ``A | B`` a union, unknown is None.

    Example:
        >>> _column_type("list[decimal.Decimal]"), _column_type("int | str"), _column_type("nope")
        (<class 'list'>, int | str, None)
        >>> _column_type("None")  # a VOID result
        <class 'NoneType'>
    """
    if python_type.startswith("list["):
        return list
    members = [PYTHON_TYPE_MAP.get(name.strip()) for name in python_type.split(" | ")]
    if None in members:
        return None
    return functools.reduce(operator.or_, members)
