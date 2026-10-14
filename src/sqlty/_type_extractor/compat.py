"""Compatibility of a resolved row type with SQL column types."""

from __future__ import annotations

import ast
import typing

if typing.TYPE_CHECKING:
    pass

from sqlty._type_extractor.ast_fields import _annotation_type, _split_type_args
from sqlty._type_extractor.info import TypeInfo, _column_type
from sqlty._type_extractor.subtype import (
    _LAX_FROM,
    _PROMOTED_FROM,
    _erase,
    _is_subtype,
    _union_members,
)


def check_type_compatibility(
    user_type: TypeInfo,
    column_name: str,
    column_python_type: str | None,
) -> bool:
    """Check if a user type's field can hold values of a SQL column type.

    Uses runtime type objects when available (``resolved_hints``), else the
    types resolved from source (``field_types``). ``Any``, unions and the
    ``int`` -> ``float`` promotion are accepted; narrower fields (``bool`` for
    an ``int`` column) are not, unless the field is ``lax`` (see `_LAX_FROM`).
    Annotations neither path can resolve or decide count as compatible.

    Args:
        user_type: The user-defined type info.
        column_name: Name of the SQL column / field to check.
        column_python_type: The inferred Python type string from SQL
            (e.g. ``"int"``, ``"datetime.datetime"``).

    Returns:
        True if types are compatible, False if they mismatch.

    Example:
        >>> from sqlty._type_extractor import extract_type_definitions
        >>> src = "import typing as t, not_installed\\nclass Row(t.NamedTuple):\\n id: int | t.Any\\n flag: bool"
        >>> info = extract_type_definitions(src)["Row"]
        >>> check_type_compatibility(info, "id", "str"), check_type_compatibility(info, "flag", "int")
        (True, False)
    """
    declared = dict(user_type.fields)
    if column_name in declared:
        annotation = declared[column_name]
        hints = user_type.resolved_hints or {}
        field = (
            hints[column_name] if column_name in hints else user_type.field_types.get(column_name)
        )
    elif user_type.extra_items is not None:
        annotation = user_type.extra_items
        field = user_type.extra_items_hint or user_type.extra_items_type
    else:
        return column_python_type is None
    lax = column_name in user_type.lax
    return _accepts(field, annotation, column_python_type, _LAX_FROM if lax else _PROMOTED_FROM)


def admits_none(tp: typing.Any) -> bool | None:
    """Whether a resolved annotation admits ``None``; ``None`` when it can't be told.

    ``Optional[X]``, ``X | None``, ``Any``, ``object`` and ``Literal[None]`` admit it;
    plain classes, generics and ``NewType`` of those don't. A forward reference or a
    name that failed to resolve is undecided.

    Example:
        >>> admits_none(int), admits_none(int | None), admits_none(typing.Any), admits_none(None)
        (False, True, True, None)
    """
    if tp is None:
        return None
    verdicts = [_member_admits_none(member) for member in _union_members(tp)]
    if True in verdicts:
        return True
    return None if None in verdicts else False


def _member_admits_none(member: typing.Any) -> bool | None:
    if member is type(None) or member is None or member is typing.Any or member is object:
        return True
    if isinstance(member, typing.NewType):
        return admits_none(member.__supertype__)
    origin = typing.get_origin(member)
    if origin is typing.Literal:
        return None in typing.get_args(member)
    if isinstance(member, type) or isinstance(origin, type):
        return False
    return None


def column_literal_values(python_type: str) -> list[str | int] | None:
    """The values of an inferred ``Literal`` column type, ``None`` when it is not one.

    ``None`` members (a nullable column) are left out; nullability is checked separately.

    Example:
        >>> column_literal_values("typing.Literal['a', 'b'] | None")
        ['a', 'b']
        >>> column_literal_values("typing.Literal[1, 2]")
        [1, 2]
        >>> column_literal_values("str") is None
        True
    """
    try:
        tree = ast.parse(python_type, mode="eval").body
    except SyntaxError:
        return None
    members = [tree]
    while any(isinstance(m, ast.BinOp) and isinstance(m.op, ast.BitOr) for m in members):
        members = [
            part
            for m in members
            for part in ((m.left, m.right) if isinstance(m, ast.BinOp) else (m,))
        ]
    values: list[str | int] = []
    for member in members:
        if isinstance(member, ast.Constant) and member.value is None:
            continue
        if not (
            isinstance(member, ast.Subscript)
            and isinstance(member.value, ast.Attribute)
            and ast.unparse(member.value) == "typing.Literal"
        ):
            return None
        items = member.slice.elts if isinstance(member.slice, ast.Tuple) else [member.slice]
        for item in items:
            if not (isinstance(item, ast.Constant) and isinstance(item.value, (str, int))):
                return None
            values.append(item.value)
    return values or None


def declared_literal_values(tp: typing.Any, inferred: list[str | int]) -> set[str | int] | None:
    """The values a resolved annotation admits for a column that takes ``inferred``.

    ``None`` when the annotation also takes values by class (``str`` for string values,
    ``int`` for integers, ``Any``), or when it can't be told (an enum or other user class,
    an unresolved name): only an annotation made of ``Literal`` members and ``None`` is a set.

    Example:
        >>> declared_literal_values(typing.Literal["a", "b"] | None, ["a"]) == {"a", "b"}
        True
        >>> declared_literal_values(str, ["a"]) is None
        True
        >>> declared_literal_values(typing.Literal[1], ["a"]) == {1}
        True
    """
    covering: tuple[type, ...] = (str,) if isinstance(inferred[0], str) else (int, float, complex)
    admitted: set[str | int] = set()
    for member in _union_members(tp):
        while isinstance(member, typing.NewType):
            member = member.__supertype__
        if member is type(None) or member is None:
            continue
        if typing.get_origin(member) is typing.Literal:
            admitted.update(a for a in typing.get_args(member) if isinstance(a, (str, int)))
        elif (
            member is typing.Any
            or member is object
            or member in covering
            or not (isinstance(member, type) and member.__module__ == "builtins")
        ):
            return None
    return admitted


def is_optional(tp: typing.Any) -> bool:
    """Whether a resolved annotation spells ``None`` out: ``Optional[X]`` or ``X | None``.

    ``Any`` and ``object`` admit ``None`` but are not optional in this sense.

    Example:
        >>> is_optional(int | None), is_optional(typing.Optional[str]), is_optional(typing.Any)
        (True, True, False)
    """
    members = _union_members(tp)
    return len(members) > 1 and any(m is type(None) for m in members)


def field_type(user_type: TypeInfo, column_name: str) -> typing.Any:
    """The resolved annotation of the field of ``user_type`` that holds ``column_name``.

    Resolved as `check_type_compatibility` resolves it, but with ``| None`` kept (type
    compatibility ignores it, since every column may be NULL); ``None`` when the field is
    unknown or its annotation can't be resolved.
    """
    declared = dict(user_type.fields)
    if column_name in declared:
        hints = user_type.declared_hints
        return (
            hints[column_name] if column_name in hints else user_type.field_types.get(column_name)
        )
    if user_type.extra_items is not None:
        return user_type.extra_items_type
    return None


def field_accepts(annotation: str, column_python_type: str | None) -> bool:
    """Check if a field annotated ``annotation`` can hold values of a SQL column type.

    For annotations known only as strings, such as ``SQL[tuple[...]]`` items;
    see `check_type_compatibility`.

    Example:
        >>> field_accepts("float", "int"), field_accepts("bool", "int"), field_accepts("Any", "str")
        (True, False, True)
    """
    return _accepts(_annotation_type(annotation), annotation, column_python_type)


def _accepts(
    field: typing.Any,
    annotation: str,
    column_python_type: str | None,
    promotions: dict[type, tuple[type, ...]] | None = None,
) -> bool:
    if not column_python_type:
        return True
    fits = _is_subtype(_column_type(column_python_type), _erase(field), promotions)
    return fits is not False


def parse_tuple_annotation(annotation: str) -> list[tuple[str, str]] | None:
    """Parse a tuple type annotation into a field list.

    Args:
        annotation: Type annotation string like ``"tuple[int, str]"``.

    Returns:
        List of (positional_name, type) pairs, or None if not a tuple.
        Positional names are ``"_0"``, ``"_1"``, etc.

    Examples:
        >>> parse_tuple_annotation("tuple[int, str]")
        [('_0', 'int'), ('_1', 'str')]
        >>> parse_tuple_annotation("tuple[int]")
        [('_0', 'int')]
        >>> parse_tuple_annotation("User")
    """
    if not annotation.startswith("tuple[") or not annotation.endswith("]"):
        return None
    inner = annotation[6:-1]
    parts = _split_type_args(inner)
    return [(f"_{i}", t.strip()) for i, t in enumerate(parts)]
