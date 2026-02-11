"""Extract field information from Python type definitions in source code.

Parses Python source to find type definitions (TypedDict, NamedTuple,
dataclass, Pydantic BaseModel) and extract their fields for structural
validation against SQL query columns.

This module enables ``check_source`` to validate ``SQL[UserType](...)``
annotations by comparing the user-defined type's fields against the
columns inferred from the SQL query.
"""

from __future__ import annotations

import ast


class TypeInfo:
    """Information about a user-defined type extracted from source."""

    __slots__ = ("name", "fields", "kind")

    def __init__(self, name: str, fields: list[tuple[str, str]], kind: str) -> None:
        self.name = name
        self.fields = fields  # [(field_name, field_type), ...]
        self.kind = kind  # "TypedDict", "NamedTuple", "dataclass", "BaseModel"

    def __repr__(self) -> str:
        return f"TypeInfo({self.name!r}, fields={self.fields!r}, kind={self.kind!r})"


def extract_type_definitions(source: str) -> dict[str, TypeInfo]:
    """Extract type definitions from Python source code.

    Finds class definitions that are:
    - ``TypedDict`` subclasses
    - ``NamedTuple`` subclasses
    - ``@dataclass``-decorated classes
    - Pydantic ``BaseModel`` subclasses

    Args:
        source: Python source code to parse.

    Returns:
        Dictionary mapping type name to TypeInfo.
    """
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return {}

    types: dict[str, TypeInfo] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef):
            info = _extract_class_info(node)
            if info:
                types[info.name] = info
    return types


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
    types = _split_type_args(inner)
    return [(f"_{i}", t.strip()) for i, t in enumerate(types)]


def _split_type_args(s: str) -> list[str]:
    """Split comma-separated type arguments, respecting nested brackets."""
    parts: list[str] = []
    depth = 0
    current: list[str] = []
    for ch in s:
        if ch == "[":
            depth += 1
            current.append(ch)
        elif ch == "]":
            depth -= 1
            current.append(ch)
        elif ch == "," and depth == 0:
            parts.append("".join(current).strip())
            current = []
        else:
            current.append(ch)
    if current:
        parts.append("".join(current).strip())
    return parts


def _extract_class_info(node: ast.ClassDef) -> TypeInfo | None:
    """Extract type info from a class definition node."""
    kind = _classify_class(node)
    if kind is None:
        return None

    fields = _extract_fields(node)
    if not fields:
        return None

    return TypeInfo(name=node.name, fields=fields, kind=kind)


def _classify_class(node: ast.ClassDef) -> str | None:
    """Classify a class as TypedDict, NamedTuple, dataclass, or BaseModel.

    Returns the kind string or None if the class is not a recognized type.
    """
    # Check base classes
    for base in node.bases:
        base_name = _get_name(base)
        if base_name == "TypedDict":
            return "TypedDict"
        if base_name == "NamedTuple":
            return "NamedTuple"
        if base_name == "BaseModel":
            return "BaseModel"

    # Check decorators for @dataclass
    for decorator in node.decorator_list:
        dec_name = _get_name(decorator)
        if dec_name == "dataclass":
            return "dataclass"

    return None


def _get_name(node: ast.expr) -> str | None:
    """Get the simple name from an AST expression.

    Handles: ``Name("X")``, ``Attribute(_, "X")``, ``Call(func, ...)``.
    """
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return node.attr
    if isinstance(node, ast.Call):
        return _get_name(node.func)
    return None


def _extract_fields(node: ast.ClassDef) -> list[tuple[str, str]]:
    """Extract annotated fields from a class body.

    Looks for ``ast.AnnAssign`` nodes (``field_name: type``) and
    converts annotations to type strings matching sqlty conventions.
    """
    fields: list[tuple[str, str]] = []
    for item in node.body:
        if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
            field_name = item.target.id
            field_type = _annotation_to_type_str(item.annotation)
            fields.append((field_name, field_type))
    return fields


def _annotation_to_type_str(annotation: ast.expr) -> str:
    """Convert an AST annotation to a Python type string.

    Uses ``ast.unparse()`` (Python 3.9+) and normalizes to match
    sqlty's type naming conventions.
    """
    raw = ast.unparse(annotation)
    return _normalize_type_str(raw)


def _normalize_type_str(type_str: str) -> str:
    """Normalize a type string to match sqlty conventions.

    Strips ``Optional[...]`` and ``| None`` wrappers since SQL columns
    are inherently nullable and sqlty uses the base type.
    """
    # "int | None" -> "int", "None | int" -> "int"
    if " | None" in type_str:
        type_str = type_str.replace(" | None", "")
    if "None | " in type_str:
        type_str = type_str.replace("None | ", "")
    # "Optional[int]" -> "int"
    if type_str.startswith("Optional[") and type_str.endswith("]"):
        type_str = type_str[9:-1]
    return type_str.strip()
