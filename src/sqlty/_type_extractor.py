"""Extract and validate user-defined types against SQL query columns.

Uses Python's runtime typing machinery (``get_type_hints()``, ``issubclass()``)
instead of string comparison for reliable structural validation. This handles
type aliases, ``Optional``, ``from datetime import datetime`` import
variations, forward references, and inheritance correctly.

Two-phase approach:
1. **AST scan**: Find recognized class definitions (TypedDict, NamedTuple,
   dataclass, BaseModel) and collect the source needed to reconstruct them.
2. **Runtime resolve**: ``exec()`` just the imports + class definition in a
   sandbox namespace, then use ``typing.get_type_hints()`` to get actual
   ``type`` objects for each field.

This module enables ``check_source`` to validate ``SQL[UserType](...)``
annotations by comparing the user-defined type's resolved field types
against the column types inferred from the SQL query.
"""

from __future__ import annotations

import ast
import datetime
import types
import typing
import uuid


class TypeInfo:
    """Information about a user-defined type extracted from source."""

    __slots__ = ("name", "fields", "kind", "resolved_hints")

    def __init__(
        self,
        name: str,
        fields: list[tuple[str, str]],
        kind: str,
        resolved_hints: dict[str, type] | None = None,
    ) -> None:
        self.name = name
        self.fields = fields  # [(field_name, field_type_str), ...] — AST fallback
        self.kind = kind  # "TypedDict", "NamedTuple", "dataclass", "BaseModel"
        self.resolved_hints = resolved_hints  # {field_name: type_object} — runtime

    def __repr__(self) -> str:
        return f"TypeInfo({self.name!r}, fields={self.fields!r}, kind={self.kind!r})"


# ---------------------------------------------------------------------------
# Mapping from sqlty's python_type strings to actual Python types
# (mirrors SQLAnalyzer.SQL_TO_PYTHON_TYPE but with real type objects)
# ---------------------------------------------------------------------------
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
}


def extract_type_definitions(source: str) -> dict[str, TypeInfo]:
    """Extract type definitions from Python source code.

    Finds class definitions that are:
    - ``TypedDict`` subclasses
    - ``NamedTuple`` subclasses
    - ``@dataclass``-decorated classes
    - Pydantic ``BaseModel`` subclasses

    For each recognized class, attempts runtime resolution of field types
    using ``typing.get_type_hints()``. Falls back to AST string extraction
    if runtime resolution fails (e.g. missing dependencies).

    Args:
        source: Python source code to parse.

    Returns:
        Dictionary mapping type name to TypeInfo.
    """
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return {}

    # Phase 1: AST scan — find recognized classes and extract import context
    recognized: list[tuple[ast.ClassDef, str]] = []  # (node, kind)
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef):
            kind = _classify_class(node)
            if kind and _has_annotations(node):
                recognized.append((node, kind))

    if not recognized:
        return {}

    # Phase 2: Collect import statements needed for namespace
    import_stmts = _collect_imports(tree)

    # Phase 3: For each class, try runtime resolution then fall back to AST
    result: dict[str, TypeInfo] = {}
    for node, kind in recognized:
        ast_fields = _extract_fields_from_ast(node)
        if not ast_fields:
            continue

        # Try runtime resolution
        resolved = _resolve_type_hints(node, kind, import_stmts, source)

        result[node.name] = TypeInfo(
            name=node.name,
            fields=ast_fields,
            kind=kind,
            resolved_hints=resolved,
        )

    return result


def check_type_compatibility(
    user_type: TypeInfo,
    column_name: str,
    column_python_type: str | None,
) -> bool:
    """Check if a user type's field is compatible with a SQL column type.

    Uses runtime type objects when available (``resolved_hints``), falling
    back to string comparison.

    Args:
        user_type: The user-defined type info.
        column_name: Name of the SQL column / field to check.
        column_python_type: The inferred Python type string from SQL
            (e.g. ``"int"``, ``"datetime.datetime"``).

    Returns:
        True if types are compatible, False if they mismatch.
    """
    if column_python_type is None:
        return True  # Can't validate unknown SQL type

    # Runtime path: use resolved type hints
    if user_type.resolved_hints and column_name in user_type.resolved_hints:
        field_type = user_type.resolved_hints[column_name]
        expected_type = PYTHON_TYPE_MAP.get(column_python_type)
        if expected_type is not None:
            return _types_compatible(field_type, expected_type)

    # Fallback: string comparison (from AST fields)
    for field_name, field_type_str in user_type.fields:
        if field_name == column_name:
            normalized = _normalize_type_str(field_type_str)
            return normalized == column_python_type

    return False  # field not found


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


# ---------------------------------------------------------------------------
# Runtime type resolution
# ---------------------------------------------------------------------------


def _resolve_type_hints(
    node: ast.ClassDef,
    kind: str,
    import_stmts: list[str],
    full_source: str,
) -> dict[str, type] | None:
    """Try to resolve a class's field types at runtime.

    Executes just the necessary imports + the class definition in a
    sandboxed namespace, then uses ``typing.get_type_hints()`` to get
    actual type objects.

    Returns None if resolution fails for any reason.
    """
    # Build minimal source: imports + class definition
    class_source = ast.get_source_segment(full_source, node)
    if class_source is None:
        return None

    # Build a safe namespace with common types pre-loaded
    namespace: dict[str, object] = {
        "__builtins__": __builtins__,
        "datetime": datetime,
        "uuid": uuid,
    }

    # Add typing imports based on kind
    exec_lines = list(import_stmts)
    exec_lines.append(class_source)
    exec_source = "\n".join(exec_lines)

    try:
        # dont_inherit=True prevents this module's `from __future__ import
        # annotations` from turning the exec'd class annotations into
        # ForwardRef strings — we need real type objects for get_type_hints.
        code = compile(exec_source, "<sqlty-type-check>", "exec", dont_inherit=True)
        exec(code, namespace)  # noqa: S102
    except Exception:
        return None

    cls = namespace.get(node.name)
    if cls is None or not isinstance(cls, type):
        return None

    try:
        hints = typing.get_type_hints(cls, globalns=namespace)
    except Exception:
        return None

    # Unwrap Optional / Union with None for each field
    resolved: dict[str, type] = {}
    for field_name, field_type in hints.items():
        resolved[field_name] = _unwrap_optional(field_type)

    return resolved


def _types_compatible(field_type: type, expected_type: type) -> bool:
    """Check if a field type is compatible with the expected SQL type.

    Handles:
    - Exact match: ``int`` == ``int``
    - Subclass: ``bool`` is subclass of ``int``
    - Union/Optional unwrapping done by caller
    """
    try:
        if field_type is expected_type:
            return True
        return issubclass(field_type, expected_type)
    except TypeError:
        # issubclass fails for some generic types
        return field_type is expected_type


def _unwrap_optional(tp: type) -> type:
    """Unwrap Optional[X] / X | None to X.

    SQL columns are inherently nullable, so we strip None from unions.
    """
    origin = typing.get_origin(tp)

    # types.UnionType (X | Y syntax, Python 3.10+)
    if origin is types.UnionType or origin is typing.Union:
        args = typing.get_args(tp)
        non_none = [a for a in args if a is not type(None)]
        if len(non_none) == 1:
            return non_none[0]

    return tp


# ---------------------------------------------------------------------------
# AST helpers
# ---------------------------------------------------------------------------


def _collect_imports(tree: ast.Module) -> list[str]:
    """Collect import statements from an AST as source strings."""
    imports: list[str] = []
    for node in ast.iter_child_nodes(tree):
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            imports.append(ast.unparse(node))
    return imports


def _has_annotations(node: ast.ClassDef) -> bool:
    """Check if a class has at least one annotated field."""
    return any(
        isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name) for item in node.body
    )


def _classify_class(node: ast.ClassDef) -> str | None:
    """Classify a class as TypedDict, NamedTuple, dataclass, or BaseModel."""
    for base in node.bases:
        base_name = _get_name(base)
        if base_name == "TypedDict":
            return "TypedDict"
        if base_name == "NamedTuple":
            return "NamedTuple"
        if base_name == "BaseModel":
            return "BaseModel"

    for decorator in node.decorator_list:
        dec_name = _get_name(decorator)
        if dec_name == "dataclass":
            return "dataclass"

    return None


def _get_name(node: ast.expr) -> str | None:
    """Get the simple name from an AST expression."""
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return node.attr
    if isinstance(node, ast.Call):
        return _get_name(node.func)
    return None


def _extract_fields_from_ast(node: ast.ClassDef) -> list[tuple[str, str]]:
    """Extract annotated fields from a class body as (name, type_str) pairs.

    This is the fallback when runtime resolution is not available.
    """
    fields: list[tuple[str, str]] = []
    for item in node.body:
        if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
            field_name = item.target.id
            field_type = ast.unparse(item.annotation)
            fields.append((field_name, _normalize_type_str(field_type)))
    return fields


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


def _normalize_type_str(type_str: str) -> str:
    """Normalize a type string — strips Optional/None wrappers.

    Used as fallback when runtime resolution is not available.
    """
    if " | None" in type_str:
        type_str = type_str.replace(" | None", "")
    if "None | " in type_str:
        type_str = type_str.replace("None | ", "")
    if type_str.startswith("Optional[") and type_str.endswith("]"):
        type_str = type_str[9:-1]
    return type_str.strip()
