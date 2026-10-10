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
    """Information about a user-defined type extracted from source.

    ``required``, ``closed`` and ``extra_items`` describe TypedDict keys
    (PEP 655, PEP 728); other kinds have every field required and are open.
    ``errors`` lists PEP 728 inheritance violations of a TypedDict subclass;
    ``bases`` names its TypedDict bases defined in the same source.
    """

    __slots__ = (
        "name",
        "fields",
        "kind",
        "resolved_hints",
        "required",
        "closed",
        "extra_items",
        "extra_items_hint",
        "extra_items_readonly",
        "extra_items_owner",
        "lineno",
        "bases",
        "errors",
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
        self.extra_items_owner: str | None = None  # class whose extra_items argument applies
        self.lineno = 0
        self.bases: list[str] = []
        self.errors: list[str] = []

    @property
    def keyed(self) -> bool:
        """Whether columns match keys by name (closed or ``extra_items`` TypedDict)."""
        return self.closed or self.extra_items is not None

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

    aliases = _typing_aliases(tree)

    # Phase 1: AST scan — find recognized classes; TypedDict subclasses
    # are recognized when their base is a TypedDict defined earlier
    recognized: list[tuple[ast.ClassDef, str]] = []  # (node, kind)
    typeddict_nodes: dict[str, ast.ClassDef] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef):
            kind = _classify_class(node, aliases, typeddict_nodes)
            if kind == "TypedDict":
                typeddict_nodes[node.name] = node
            if kind:
                recognized.append((node, kind))

    if not recognized:
        return {}

    # Phase 2: Collect import statements needed for namespace
    import_stmts = _collect_imports(tree)

    # Phase 3: For each class, try runtime resolution then fall back to AST
    result: dict[str, TypeInfo] = {}
    typeddicts: dict[str, TypeInfo] = {}
    for node, kind in recognized:
        keywords = {k.arg: k.value for k in node.keywords if k.arg}
        total = not (isinstance(t := keywords.get("total"), ast.Constant) and t.value is False)
        own_fields, own_required = _extract_fields_from_ast(node, aliases, total=total)
        parents = [typeddicts[n] for n in _base_names(node) if n in typeddicts]
        inherited = {k: v for p in parents for k, v in p.fields}
        fields = list((inherited | dict(own_fields)).items())
        required = frozenset(
            {k for p in parents for k in p.required} - dict(own_fields).keys() | own_required
        )

        info = TypeInfo(name=node.name, fields=fields, kind=kind, required=required)
        info.lineno = node.lineno
        new_keys = [k for k, _ in own_fields if k not in inherited]
        if kind == "TypedDict":
            info.bases = [p.name for p in parents]
            _apply_openness(info, keywords, aliases, parents, new_keys)
            typeddicts[node.name] = info
        if not fields:
            continue

        nodes = list(_lineage(node, typeddict_nodes).values()) if kind == "TypedDict" else [node]
        if resolved := _resolve_class(nodes, import_stmts, source):
            cls, namespace = resolved
            info.resolved_hints = _resolve_type_hints(cls, namespace)
            if kind == "TypedDict" and info.resolved_hints is not None:
                info.required = frozenset(getattr(cls, "__required_keys__", info.required))
                _check_extra_items_types(info, cls, namespace, parents, new_keys)
                owner = namespace.get(info.extra_items_owner or "")
                if (extra := getattr(owner, "__extra_items__", None)) is not None:
                    info.extra_items_hint = _unwrap_optional(_unwrap_qualifiers(extra))

        result[node.name] = info

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

    expected_type = PYTHON_TYPE_MAP.get(column_python_type)
    declared = dict(user_type.fields)

    if column_name not in declared:
        if user_type.extra_items is None:
            return False
        if user_type.extra_items_hint is not None and expected_type is not None:
            return _types_compatible(user_type.extra_items_hint, expected_type)
        return user_type.extra_items == column_python_type

    # Runtime path: use resolved type hints
    if user_type.resolved_hints and column_name in user_type.resolved_hints:
        if expected_type is not None:
            return _types_compatible(user_type.resolved_hints[column_name], expected_type)

    # Fallback: string comparison (from AST fields)
    return _normalize_type_str(declared[column_name]) == column_python_type


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


def _resolve_class(
    nodes: list[ast.ClassDef],
    import_stmts: list[str],
    full_source: str,
) -> tuple[type, dict[str, object]] | None:
    """Try to build the last of ``nodes`` at runtime, returning it with its namespace.

    Executes just the necessary imports + the class definitions (bases
    first) in a sandboxed namespace. Returns None if this fails for any reason.
    """
    class_sources = [
        segment for node in nodes if (segment := ast.get_source_segment(full_source, node))
    ]
    if len(class_sources) != len(nodes):
        return None

    # Build a safe namespace with common types pre-loaded
    namespace: dict[str, object] = {
        "__builtins__": __builtins__,
        "datetime": datetime,
        "uuid": uuid,
    }

    # Add typing imports based on kind
    exec_lines = [*import_stmts, *class_sources]
    exec_source = "\n".join(exec_lines)

    try:
        # dont_inherit=True prevents this module's `from __future__ import
        # annotations` from turning the exec'd class annotations into
        # ForwardRef strings — we need real type objects for get_type_hints.
        code = compile(exec_source, "<sqlty-type-check>", "exec", dont_inherit=True)
        exec(code, namespace)  # noqa: S102
    except Exception:
        return None

    cls = namespace.get(nodes[-1].name)
    return (cls, namespace) if isinstance(cls, type) else None


def _resolve_type_hints(cls: type, namespace: dict[str, object]) -> dict[str, type] | None:
    """Resolve field types, dropping qualifiers (``ReadOnly`` etc.) and ``None``."""
    try:
        hints = typing.get_type_hints(cls, globalns=namespace)
    except Exception:
        return None
    return {name: _unwrap_optional(tp) for name, tp in hints.items()}


_QUALIFIERS = frozenset({"ReadOnly", "Required", "NotRequired"})


def _unwrap_qualifiers(tp: typing.Any) -> typing.Any:
    """Strip ``ReadOnly`` / ``Required`` / ``NotRequired`` wrappers."""
    while typing.get_origin(tp) in (typing.ReadOnly, typing.Required, typing.NotRequired):
        tp = typing.get_args(tp)[0]
    return tp


def _check_extra_items_types(
    info: TypeInfo,
    cls: type,
    namespace: dict[str, object],
    parents: list[TypeInfo],
    new_keys: list[str],
) -> None:
    """Check items and ``extra_items`` a subclass adds against a base's ``extra_items``.

    Per PEP 728, they must be assignable to a read-only base ``extra_items``
    and consistent with a mutable one. A rejected ``extra_items`` override
    keeps the base's.
    """
    try:
        hints = typing.get_type_hints(cls, globalns=namespace)
    except Exception:
        return
    own = cls.__dict__.get("__extra_items__", typing.NoExtraItems)
    own = None if own is typing.NoExtraItems else _unwrap_qualifiers(own)
    for p in parents:
        base_cls = namespace.get(p.extra_items_owner or "")
        if (base := getattr(base_cls, "__extra_items__", None)) is None:
            continue
        base = _unwrap_qualifiers(base)
        fits = _assignable if p.extra_items_readonly else _consistent
        what = f"{'read-only ' if p.extra_items_readonly else ''}extra_items {_type_repr(base)}"
        info.errors += [
            f"{info.name}.{key} is {_type_repr(hints[key])} but base {p.name} has {what}"
            for key in new_keys
            if key in hints and not fits(hints[key], base)
        ]
        if own is not None and own not in _NEVER and not fits(own, base):
            info.errors.append(
                f"{info.name} extra_items {_type_repr(own)} doesn't fit base {p.name} {what}"
            )
            info.closed, info.extra_items = p.closed, p.extra_items
            info.extra_items_readonly, info.extra_items_owner = (
                p.extra_items_readonly,
                p.extra_items_owner,
            )


_NEVER = (typing.Never, typing.NoReturn)


def _union_members(tp: typing.Any) -> set[typing.Any]:
    if typing.get_origin(tp) in (typing.Union, types.UnionType):
        return set(typing.get_args(tp))
    return {tp}


def _consistent(src: typing.Any, dst: typing.Any) -> bool:
    return typing.Any in (src, dst) or _union_members(src) == _union_members(dst)


def _assignable(src: typing.Any, dst: typing.Any) -> bool:
    """Whether every member of ``src`` is a member or subclass of one in ``dst``."""

    def fits(s: typing.Any, d: typing.Any) -> bool:
        if s == d or d in (object, typing.Any) or s is typing.Any or s in _NEVER:
            return True
        try:
            return isinstance(s, type) and isinstance(d, type) and issubclass(s, d)
        except TypeError:
            return False

    targets = _union_members(dst)
    return all(any(fits(s, d) for d in targets) for s in _union_members(src))


def _type_repr(tp: typing.Any) -> str:
    if isinstance(tp, type) and not typing.get_args(tp):
        return tp.__name__
    return repr(tp).replace("typing.", "")


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
            return non_none[0]  # type: ignore[no-any-return]

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


def _typing_aliases(tree: ast.Module) -> dict[str, str]:
    """Map local names imported from ``typing`` / ``typing_extensions`` to originals."""
    return {
        alias.asname: alias.name
        for node in ast.walk(tree)
        if isinstance(node, ast.ImportFrom) and node.module in ("typing", "typing_extensions")
        for alias in node.names
        if alias.asname
    }


def _classify_class(
    node: ast.ClassDef,
    aliases: dict[str, str] | None = None,
    typeddicts: typing.Container[str] = (),
) -> str | None:
    """Classify a class as TypedDict, NamedTuple, dataclass, or BaseModel.

    Subclasses of the names in ``typeddicts`` are TypedDicts too.
    """
    aliases = aliases or {}
    for base in node.bases:
        base_name = _get_name(base)
        base_name = aliases.get(base_name, base_name) if base_name else None
        if base_name == "TypedDict" or (isinstance(base, ast.Name) and base.id in typeddicts):
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


def _base_names(node: ast.ClassDef) -> list[str]:
    """Names of bases spelled as bare names."""
    return [base.id for base in node.bases if isinstance(base, ast.Name)]


def _lineage(node: ast.ClassDef, classes: dict[str, ast.ClassDef]) -> dict[str, ast.ClassDef]:
    """``node`` and its earlier-defined bases among ``classes``, bases first."""
    lineage: dict[str, ast.ClassDef] = {}
    for name in _base_names(node):
        if (base := classes.get(name)) is not None and base.lineno < node.lineno:
            lineage |= _lineage(base, classes)
    return lineage | {node.name: node}


def _apply_openness(
    info: TypeInfo,
    keywords: dict[str, ast.expr],
    aliases: dict[str, str],
    parents: list[TypeInfo],
    new_keys: list[str],
) -> None:
    """Set ``closed`` / ``extra_items`` per PEP 728, recording violations in ``errors``.

    Without ``closed`` or ``extra_items`` the first keyed base's are inherited.
    ``extra_items=Never`` (or ``NoReturn``) is ``closed=True``. A subclass of a
    closed base must stay closed and add no keys; ``closed=False`` cannot
    reopen a closed or ``extra_items`` base; a mutable base ``extra_items``
    must be kept and only admits new ``NotRequired`` keys. An invalid
    override keeps the base's openness.
    """
    own: tuple[bool, str | None, bool] | None = None  # (closed, extra_items, read-only)
    closed_kw = keywords.get("closed")
    reopen = isinstance(closed_kw, ast.Constant) and closed_kw.value is False
    if (extra_kw := keywords.get("extra_items")) is not None:
        readonly = isinstance(extra_kw, ast.Subscript) and _is_name(
            extra_kw.value, "ReadOnly", aliases
        )
        extra = _unwrap_qualifiers_ast(extra_kw, aliases)[0]
        if _is_name(extra, "Never", aliases) or _is_name(extra, "NoReturn", aliases):
            own = (True, None, False)
        else:
            own = (False, _normalize_type_str(ast.unparse(extra)), readonly)
    elif isinstance(closed_kw, ast.Constant) and isinstance(closed_kw.value, bool):
        own = (closed_kw.value, None, False)

    keyed = [p for p in parents if p.keyed]
    for p in keyed:
        mutable_extra = p.extra_items is not None and not p.extra_items_readonly
        if reopen:
            what = "is closed" if p.closed else "has extra_items"
            info.errors.append(f"{info.name} cannot set closed=False: base {p.name} {what}")
        elif p.closed and own is not None and not own[0]:
            info.errors.append(f"{info.name} must stay closed: base {p.name} is closed")
        elif mutable_extra and own is not None and own[0]:
            info.errors.append(f"{info.name} must keep base {p.name} extra_items {p.extra_items}")
    header_invalid = bool(info.errors)
    for p in keyed:
        for key in new_keys:
            if p.closed:
                info.errors.append(f"{info.name} cannot add key '{key}' to closed base {p.name}")
            elif not p.extra_items_readonly and key in info.required:
                info.errors.append(
                    f"{info.name}.{key} must be NotRequired: "
                    f"base {p.name} has extra_items {p.extra_items}"
                )

    if own is None or header_invalid:
        if keyed:
            base = keyed[0]
            info.closed, info.extra_items = base.closed, base.extra_items
            info.extra_items_readonly = base.extra_items_readonly
            info.extra_items_owner = base.extra_items_owner
        return
    info.closed, info.extra_items, info.extra_items_readonly = own
    info.extra_items_owner = info.name if own[1] is not None else None


def _is_name(node: ast.expr, name: str, aliases: dict[str, str]) -> bool:
    return (n := _get_name(node)) is not None and aliases.get(n, n) == name


def _get_name(node: ast.expr) -> str | None:
    """Get the simple name from an AST expression."""
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return node.attr
    if isinstance(node, ast.Call):
        return _get_name(node.func)
    return None


def _extract_fields_from_ast(
    node: ast.ClassDef,
    aliases: dict[str, str] | None = None,
    *,
    total: bool = True,
) -> tuple[list[tuple[str, str]], frozenset[str]]:
    """Extract annotated fields as (name, type_str) pairs plus required names.

    Qualifiers are stripped from the type strings. This is the fallback
    when runtime resolution is not available.
    """
    fields: list[tuple[str, str]] = []
    required: set[str] = set()
    for item in node.body:
        if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
            annotation, is_required = _unwrap_qualifiers_ast(item.annotation, aliases or {})
            fields.append((item.target.id, _normalize_type_str(ast.unparse(annotation))))
            if total if is_required is None else is_required:
                required.add(item.target.id)
    return fields, frozenset(required)


def _unwrap_qualifiers_ast(
    annotation: ast.expr, aliases: dict[str, str]
) -> tuple[ast.expr, bool | None]:
    """Strip qualifier subscripts; report ``Required`` (True) / ``NotRequired`` (False)."""
    required: bool | None = None
    while isinstance(annotation, ast.Subscript):
        name = _get_name(annotation.value)
        name = aliases.get(name, name) if name else None
        if name not in _QUALIFIERS:
            break
        if name != "ReadOnly":
            required = name == "Required"
        annotation = annotation.slice
    return annotation, required


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
