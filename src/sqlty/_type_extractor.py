"""Extract and validate user-defined types against SQL query columns.

Uses Python's runtime typing machinery (``get_type_hints()``, ``issubclass()``)
instead of string comparison for reliable structural validation. This handles
type aliases, ``Optional``, ``from datetime import datetime`` import
variations, forward references, and inheritance correctly.

Two-phase approach:
1. **AST scan**: Find recognized class definitions (TypedDict, NamedTuple,
   dataclass, BaseModel) and collect the source needed to reconstruct them.
2. **Runtime resolve**: ``exec()`` just the imports, module-level type
   aliases and ``NewType`` assignments (each guarded: a name that fails to
   import or evaluate becomes an unknown placeholder) and the class
   definition in a sandbox namespace, then use ``typing.get_type_hints()`` on
   each field on its own to get actual ``type`` objects. A field that stays
   unresolved is unknown and never reported.

This module enables ``check_source`` to validate ``SQL[UserType](...)``
annotations by comparing the user-defined type's resolved field types
against the column types inferred from the SQL query.
"""

from __future__ import annotations

import annotationlib
import ast
import datetime
import functools
import operator
import textwrap
import types
import typing
import uuid

from sqlty._source import ParsedSource


class TypeInfo:
    """Information about a user-defined type extracted from source.

    ``required``, ``closed`` and ``extra_items`` describe TypedDict keys
    (PEP 655, PEP 728); other kinds have every field required and are open.
    For TypedDicts, ``extra_items_readonly`` and ``extra_items_type`` (the
    resolved type, or None when unknown) complete ``extra_items``. ``field_types``
    holds each field's type resolved from source (None when unknown), ``owner``
    names the class whose header set the openness, ``lineno`` is the class
    line, ``bases`` names its TypedDict bases defined in the same source and
    ``errors`` lists its PEP 728 inheritance violations.
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
        "extra_items_type",
        "field_types",
        "owner",
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
        self.extra_items_type: typing.Any = None
        self.field_types: dict[str, typing.Any] = {}
        self.owner = name
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
# (mirrors SQL_TO_PYTHON_TYPE in rust/src/analyzer.rs, with real type objects)
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


def extract_type_definitions(source: str | ParsedSource) -> dict[str, TypeInfo]:
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
        source: Python source code, or a ``ParsedSource`` whose tree is reused.

    Returns:
        Dictionary mapping type name to TypeInfo.
    """
    parsed = ParsedSource.of(source)
    if (tree := parsed.tree) is None:
        return {}

    aliases = parsed.typing_aliases
    newtypes = _newtype_assignments(tree, aliases)

    # Phase 1: AST scan — find recognized classes; TypedDict subclasses
    # are recognized when their base is a TypedDict defined earlier
    recognized: list[tuple[ast.ClassDef, str]] = []  # (node, kind)
    typeddict_nodes: dict[str, ast.ClassDef] = {}
    for node in parsed.classes:
        kind = _classify_class(node, aliases, typeddict_nodes)
        if kind == "TypedDict":
            typeddict_nodes[node.name] = node
        if kind:
            recognized.append((node, kind))

    if not recognized:
        return {}

    # Phase 2: Collect the guarded imports and aliases needed for the namespace
    import_stmts = _namespace_statements(tree, set(newtypes))

    # Phase 3: For each class, try runtime resolution then fall back to AST
    result: dict[str, TypeInfo] = {}
    typeddicts: dict[str, TypeInfo] = {}
    for node, kind in recognized:
        keywords = {k.arg: k.value for k in node.keywords if k.arg}
        total = not (isinstance(t := keywords.get("total"), ast.Constant) and t.value is False)
        own_fields, own_required = _extract_fields_from_ast(
            node, aliases, total=total, defaults_optional=kind in ("dataclass", "BaseModel")
        )
        parents = [typeddicts[n] for n in _base_names(node) if n in typeddicts]
        inherited = {k: v for p in parents for k, v in p.fields}
        fields = list((inherited | dict(own_fields)).items())
        required = frozenset(
            {k for p in parents for k in p.required} - dict(own_fields).keys() | own_required
        )

        info = TypeInfo(name=node.name, fields=fields, kind=kind, required=required)
        info.lineno = node.lineno
        is_typeddict = kind == "TypedDict"
        nodes = list(_lineage(node, typeddict_nodes).values()) if is_typeddict else [node]
        info.field_types = _ast_key_types(nodes, aliases, newtypes)
        hints = None
        if (fields or is_typeddict) and (
            resolved := _resolve_class(nodes, import_stmts, parsed.text)
        ):
            cls, namespace = resolved
            hints = _type_hints(cls, namespace)
            info.resolved_hints = {name: _unwrap_optional(tp) for name, tp in hints.items()}
        if is_typeddict:
            info.bases = [p.name for p in parents]
            own = _own_openness(keywords, aliases)
            if hints is not None:
                info.required = frozenset(getattr(cls, "__required_keys__", info.required))
                own_type = _own_extra_type(cls, namespace) if own and own.extra_items else None
            else:
                hints = info.field_types
                own_type = (
                    _ast_type(own.extra_node, aliases, newtypes) if own and own.extra_node else None
                )
            header = _header_errors(info.name, keywords, own, own_type, parents)
            info.errors = list(dict.fromkeys(header + _key_errors(info, hints, parents)))
            _set_openness(info, None if header else own, own_type, parents)
            if info.resolved_hints is not None and info.extra_items_type is not None:
                info.extra_items_hint = _unwrap_optional(info.extra_items_type)
            typeddicts[node.name] = info
        if fields:
            result[node.name] = info

    return result


def check_type_compatibility(
    user_type: TypeInfo,
    column_name: str,
    column_python_type: str | None,
) -> bool:
    """Check if a user type's field can hold values of a SQL column type.

    Uses runtime type objects when available (``resolved_hints``), else the
    types resolved from source (``field_types``). ``Any``, unions and the
    ``int`` -> ``float`` promotion are accepted; narrower fields (``bool`` for
    an ``int`` column) are not. Annotations neither path can resolve or decide
    count as compatible.

    Args:
        user_type: The user-defined type info.
        column_name: Name of the SQL column / field to check.
        column_python_type: The inferred Python type string from SQL
            (e.g. ``"int"``, ``"datetime.datetime"``).

    Returns:
        True if types are compatible, False if they mismatch.

    Example:
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
    return _accepts(field, annotation, column_python_type)


def field_accepts(annotation: str, column_python_type: str | None) -> bool:
    """Check if a field annotated ``annotation`` can hold values of a SQL column type.

    For annotations known only as strings, such as ``SQL[tuple[...]]`` items;
    see `check_type_compatibility`.

    Example:
        >>> field_accepts("float", "int"), field_accepts("bool", "int"), field_accepts("Any", "str")
        (True, False, True)
    """
    return _accepts(_annotation_type(annotation), annotation, column_python_type)


def _accepts(field: typing.Any, annotation: str, column_python_type: str | None) -> bool:
    if not column_python_type:
        return True
    fits = _is_subtype(PYTHON_TYPE_MAP.get(column_python_type), _erase(field))
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


# ---------------------------------------------------------------------------
# Runtime type resolution
# ---------------------------------------------------------------------------


class _Unknown:
    """Stands in for a name that failed to import or evaluate; every operation yields it again."""

    def __or__(self, other: object) -> _Unknown:
        return self

    __ror__ = __or__

    def __getitem__(self, key: object) -> _Unknown:
        return self

    def __call__(self, *args: object, **kwargs: object) -> _Unknown:
        return self

    def __getattr__(self, name: str) -> _Unknown:
        if name.startswith("__"):
            raise AttributeError(name)
        return self


_UNKNOWN_NAME = "__sqlty_unknown__"


def _guard(statement: ast.stmt, names: list[str]) -> str:
    """``statement`` as source that binds ``names`` to the unknown placeholder if it fails."""
    body = textwrap.indent(ast.unparse(statement), "    ")
    fallback = "\n".join(f"    {name} = {_UNKNOWN_NAME}" for name in names)
    return f"try:\n{body}\nexcept Exception:\n{fallback}"


def _namespace_statements(tree: ast.Module, newtypes: set[str]) -> list[str]:
    """Guarded source, in file order, of the module-level imports and type aliases.

    A name that cannot be imported or evaluated is bound to the unknown
    placeholder, so one failure does not hide the other names.

    Example:
        >>> _namespace_statements(ast.parse("import a.b\\nX = int\\nY = f()"), set())[0].splitlines()[:2]
        ['try:', '    import a.b']
    """
    statements: list[str] = []
    for node in tree.body:
        if isinstance(node, ast.Import):
            for alias in node.names:
                bound = alias.asname or alias.name.split(".")[0]
                statements.append(_guard(ast.Import(names=[alias]), [bound]))
        elif isinstance(node, ast.ImportFrom) and node.module != "__future__":
            for alias in node.names:
                if alias.name != "*":
                    from_ = ast.ImportFrom(node.module, [alias], node.level)
                    statements.append(_guard(from_, [alias.asname or alias.name]))
        elif isinstance(node, ast.TypeAlias) and isinstance(node.name, ast.Name):
            statements.append(_guard(node, [node.name.id]))
        elif (
            isinstance(node, ast.Assign)
            and len(node.targets) == 1
            and isinstance(node.targets[0], ast.Name)
            and (
                node.targets[0].id in newtypes
                or all(isinstance(n, _TYPE_EXPR_NODES) for n in ast.walk(node.value))
            )
        ):
            statements.append(_guard(node, [node.targets[0].id]))
        elif (
            isinstance(node, ast.AnnAssign)
            and isinstance(node.target, ast.Name)
            and node.value is not None
            and _get_name(node.annotation) == "TypeAlias"
            and all(isinstance(n, _TYPE_EXPR_NODES) for n in ast.walk(node.value))
        ):
            statements.append(_guard(node, [node.target.id]))
    return statements


def _resolve_class(
    nodes: list[ast.ClassDef],
    import_stmts: list[str],
    full_source: str,
) -> tuple[type, dict[str, object]] | None:
    """Try to build the last of ``nodes`` at runtime, returning it with its namespace.

    Executes just the necessary imports and aliases + the class definitions
    (bases first) in a sandboxed namespace. Returns None if the classes
    cannot be built.
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
        _UNKNOWN_NAME: _Unknown(),
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


def _has_unknown(tp: typing.Any) -> bool:
    """Whether ``tp`` is, or contains, an unresolved name or forward reference."""
    if isinstance(tp, (_Unknown, annotationlib.ForwardRef)):
        return True
    if isinstance(tp, (list, tuple)):
        return any(map(_has_unknown, tp))
    return typing.get_origin(tp) is not typing.Literal and any(
        map(_has_unknown, typing.get_args(tp))
    )


def _resolve_alias(tp: typing.Any) -> typing.Any:
    """Look through ``type X = ...`` aliases, also within unions."""
    while isinstance(tp, typing.TypeAliasType):
        tp = tp.__value__
    if len(members := _union_members(tp)) > 1:
        return functools.reduce(operator.or_, map(_resolve_alias, members))
    return tp


def _type_hints(cls: type, namespace: dict[str, object]) -> dict[str, typing.Any]:
    """Resolve each field's type on its own, with qualifiers (``ReadOnly`` etc.) dropped.

    Fields that cannot be resolved are left out.
    """
    hints: dict[str, typing.Any] = {}
    for name, raw in annotationlib.get_annotations(
        cls, format=annotationlib.Format.FORWARDREF
    ).items():
        holder = type("_Field", (), {"__annotations__": {name: raw}})
        try:
            tp = _resolve_alias(
                typing.get_type_hints(holder, globalns=namespace, localns=namespace)[name]
            )
        except Exception:
            continue
        if not _has_unknown(tp):
            hints[name] = tp
    return hints


_QUALIFIERS = frozenset({"ReadOnly", "Required", "NotRequired"})


def _unwrap_qualifiers(tp: typing.Any) -> typing.Any:
    """Strip ``ReadOnly`` / ``Required`` / ``NotRequired`` wrappers."""
    while typing.get_origin(tp) in (typing.ReadOnly, typing.Required, typing.NotRequired):
        tp = typing.get_args(tp)[0]
    return tp


def _own_extra_type(cls: type, namespace: dict[str, object]) -> typing.Any:
    """The ``extra_items`` type ``cls`` declares, forward references resolved, or None."""
    tp = _unwrap_qualifiers(cls.__dict__.get("__extra_items__"))
    if isinstance(tp, str):
        tp = annotationlib.ForwardRef(tp)
    if isinstance(tp, annotationlib.ForwardRef):
        try:
            tp = typing.evaluate_forward_ref(tp, globals=namespace, locals=namespace)
        except Exception:
            return None
    return None if _has_unknown(tp) else type(None) if tp is None else tp


_NEVER = (typing.Never, typing.NoReturn)
_PROMOTED_FROM: dict[type, tuple[type, ...]] = {float: (int,), complex: (int, float)}


def _union_members(tp: typing.Any) -> list[typing.Any]:
    if typing.get_origin(tp) in (typing.Union, types.UnionType):
        return list(typing.get_args(tp))
    return [tp]


def _is_plain(tp: typing.Any) -> bool:
    return isinstance(tp, type) and not typing.get_args(tp)


def _is_subtype(src: typing.Any, dst: typing.Any) -> bool | None:
    """Whether ``src`` is assignable to ``dst``; None when that can't be decided.

    Only plain classes and unions of them are judged, by subclassing plus the
    ``int`` -> ``float`` -> ``complex`` promotion. Generics, literals, forward
    references and unknown (None) types are undecided.

    Example:
        >>> _is_subtype(bool, int | str), _is_subtype(int, float), _is_subtype(float, int)
        (True, True, False)
        >>> _is_subtype(list[int], list) is None
        True
    """
    if src is None or dst is None:
        return None
    if src is typing.Any or dst in (typing.Any, object) or src in _NEVER:
        return True
    targets = _union_members(dst)
    result: bool | None = True
    for member in _union_members(src):
        fits = _member_fits(member, targets)
        if fits is False:
            return False
        if fits is None:
            result = None
    return result


def _erase(tp: typing.Any) -> typing.Any:
    """Unwrap ``NewType`` to its supertype and ``list[str]`` to ``list``, since inferred column types are bare.

    Example:
        >>> _erase(list[str] | typing.NewType("UserId", int))
        list | int
    """
    while isinstance(tp, typing.NewType):
        tp = tp.__supertype__
    if len(members := _union_members(tp)) > 1:
        return functools.reduce(operator.or_, map(_erase, members))
    origin = typing.get_origin(tp)
    return origin if isinstance(origin, type) else tp


def _member_fits(src: typing.Any, targets: list[typing.Any]) -> bool | None:
    # ``typing.Any`` is a class on 3.11+, so it must be caught before ``issubclass``.
    if src is typing.Any or src in _NEVER:
        return True
    if any(src == d or d in (typing.Any, object) for d in targets):
        return True
    if not _is_plain(src):
        return None
    decided = True
    for dst in targets:
        if dst in _NEVER:
            continue
        if not _is_plain(dst):
            decided = False
            continue
        try:
            if issubclass(src, (dst, *_PROMOTED_FROM.get(dst, ()))):
                return True
        except TypeError:
            decided = False
    return False if decided else None


def _consistent(src: typing.Any, dst: typing.Any) -> bool | None:
    """Whether ``src`` and ``dst`` are equivalent (mutable ``extra_items``); None if unsure.

    Example:
        >>> _consistent(int | None, None | int), _consistent(bool, int)
        (True, False)
    """
    if typing.Any in (src, dst):
        return True
    forward, backward = _is_subtype(src, dst), _is_subtype(dst, src)
    if forward is False or backward is False:
        return False
    return True if forward and backward else None


def _type_repr(tp: typing.Any) -> str:
    if isinstance(tp, type) and not typing.get_args(tp):
        return tp.__name__
    return repr(tp).replace("typing.", "")


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


_TYPING_MODULES = frozenset({"typing", "typing_extensions"})


_TYPE_EXPR_NODES = (
    ast.Name,
    ast.Attribute,
    ast.Subscript,
    ast.Tuple,
    ast.Constant,
    ast.BinOp,
    ast.BitOr,
    ast.expr_context,
)


def _newtype_assignments(tree: ast.Module, aliases: dict[str, str]) -> dict[str, ast.Assign]:
    """Module-level ``X = NewType("X", T)`` with ``T`` a type expression, by name.

    Example:
        >>> list(_newtype_assignments(ast.parse("U = NewType('U', int)\\nV = NewType('V', f())"), {}))
        ['U']
    """
    return {
        node.targets[0].id: node
        for node in tree.body
        if isinstance(node, ast.Assign)
        and len(node.targets) == 1
        and isinstance(node.targets[0], ast.Name)
        and isinstance(node.value, ast.Call)
        and _is_name(node.value.func, "NewType", aliases)
        and len(node.value.args) == 2
        and not node.value.keywords
        and isinstance(name := node.value.args[0], ast.Constant)
        and name.value == node.targets[0].id
        and all(isinstance(n, _TYPE_EXPR_NODES) for n in ast.walk(node.value.args[1]))
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


class _Openness(typing.NamedTuple):
    """``closed`` / ``extra_items`` as written in a TypedDict header."""

    spelling: str  # e.g. "closed=True", "extra_items=ReadOnly[int]"
    closed: bool
    extra_items: str | None = None
    readonly: bool = False
    extra_node: ast.expr | None = None


def _own_openness(keywords: dict[str, ast.expr], aliases: dict[str, str]) -> _Openness | None:
    """The openness a class header declares; ``extra_items=Never`` is ``closed=True``."""
    if (kw := keywords.get("extra_items")) is not None:
        spelling = f"extra_items={ast.unparse(kw)}"
        extra = _unwrap_qualifiers_ast(kw, aliases)[0]
        if _is_name(extra, "Never", aliases) or _is_name(extra, "NoReturn", aliases):
            return _Openness(spelling, closed=True)
        readonly = isinstance(kw, ast.Subscript) and _is_name(kw.value, "ReadOnly", aliases)
        return _Openness(spelling, False, _normalize_type_str(ast.unparse(extra)), readonly, extra)
    if isinstance(kw := keywords.get("closed"), ast.Constant) and isinstance(kw.value, bool):
        return _Openness(f"closed={kw.value}", kw.value)
    return None


def _extra_repr(info: TypeInfo) -> str:
    if info.extra_items_type is None:
        return str(info.extra_items)
    return _type_repr(info.extra_items_type)


def _header_errors(
    name: str,
    keywords: dict[str, ast.expr],
    own: _Openness | None,
    own_type: typing.Any,
    parents: list[TypeInfo],
) -> list[str]:
    """PEP 728 violations of a subclass header against its closed / ``extra_items`` bases.

    ``closed=False`` cannot reopen them, a closed base admits no ``extra_items``,
    a mutable base ``extra_items`` must be kept as is, and a read-only one may
    only be narrowed or closed.
    """
    closed_kw = keywords.get("closed")
    reopen = isinstance(closed_kw, ast.Constant) and closed_kw.value is False
    errors: list[str] = []
    for p in parents:
        if not p.keyed or own is None:
            continue
        base = f"base {p.owner}"
        if reopen:
            what = "is closed" if p.closed else "has extra_items"
            errors.append(f"{name} cannot set closed=False: {base} {what}")
        elif p.closed and not own.closed:
            errors.append(f"{name} cannot set {own.spelling}: {base} is closed")
        elif p.extra_items is not None and not p.extra_items_readonly:
            if own.closed or own.readonly or _consistent(own_type, p.extra_items_type) is False:
                errors.append(
                    f"{name} cannot set {own.spelling}: {base} has mutable extra_items "
                    f"{_extra_repr(p)}"
                )
        elif p.extra_items_readonly and _is_subtype(own_type, p.extra_items_type) is False:
            errors.append(
                f"{name} cannot set {own.spelling}: {base} has read-only extra_items "
                f"{_extra_repr(p)}"
            )
    return errors


def _key_errors(
    info: TypeInfo, key_types: dict[str, typing.Any], parents: list[TypeInfo]
) -> list[str]:
    """PEP 728 violations of the keys ``info`` has beyond each closed / ``extra_items`` base.

    Keys brought in by a sibling base count. A closed base admits no new keys;
    under a read-only ``extra_items`` they must be assignable to it, under a
    mutable one ``NotRequired`` and consistent with it.
    """
    errors: list[str] = []
    for p in parents:
        if not p.keyed:
            continue
        base_keys = dict(p.fields)
        for key in (k for k, _ in info.fields if k not in base_keys):
            tp = key_types.get(key)
            if p.closed:
                errors.append(f"{info.name} cannot add key '{key}' to closed base {p.owner}")
                continue
            what = f"extra_items {_extra_repr(p)}"
            if p.extra_items_readonly:
                fits = _is_subtype(tp, p.extra_items_type)
                what = f"read-only {what}"
            else:
                fits = _consistent(tp, p.extra_items_type)
                if key in info.required:
                    errors.append(
                        f"{info.name}.{key} must be NotRequired: base {p.owner} has {what}"
                    )
            if fits is False:
                errors.append(
                    f"{info.name}.{key} is {_type_repr(tp)} but base {p.owner} has {what}"
                )
    return errors


def _set_openness(
    info: TypeInfo, own: _Openness | None, own_type: typing.Any, parents: list[TypeInfo]
) -> None:
    """Take a valid header's openness, else the first closed / ``extra_items`` base's."""
    if own is not None:
        info.closed, info.extra_items, info.extra_items_readonly = (
            own.closed,
            own.extra_items,
            own.readonly,
        )
        info.extra_items_type = own_type if own.extra_items is not None else None
    elif keyed := [p for p in parents if p.keyed]:
        base = keyed[0]
        info.closed, info.extra_items = base.closed, base.extra_items
        info.extra_items_readonly, info.extra_items_type = (
            base.extra_items_readonly,
            base.extra_items_type,
        )
        info.owner = base.owner


_GENERICS = (list, dict, set, frozenset, tuple)
_AST_TYPES: dict[str, typing.Any] = {
    "int": int,
    "float": float,
    "complex": complex,
    "str": str,
    "bytes": bytes,
    "bool": bool,
    "object": object,
    "list": list,
    "dict": dict,
    "set": set,
    "frozenset": frozenset,
    "tuple": tuple,
    "Any": typing.Any,
    "Never": typing.Never,
    "NoReturn": typing.Never,
}
_TYPING_GENERICS: dict[str, typing.Any] = {
    "List": list,
    "Dict": dict,
    "Set": set,
    "FrozenSet": frozenset,
    "Tuple": tuple,
}


def _ast_type(
    node: ast.expr, aliases: dict[str, str], newtypes: dict[str, ast.Assign] | None = None
) -> typing.Any:
    """Builtin scalars and generics, ``Any``, ``Never``, NewTypes and their unions from source; None if unknown.

    ``aliases`` maps local names to ``typing`` names (see ``ParsedSource.typing_aliases``), so
    ``A`` for ``from typing import Any as A`` and ``t.Any`` for
    ``import typing as t`` resolve; ``Annotated`` metadata is dropped.
    ``typing`` generic aliases (``List``, ``Dict``, ``Set``, ``FrozenSet``,
    ``Tuple``) resolve to their builtins only when imported from ``typing``
    or ``typing_extensions``.
    ``newtypes`` maps names to their ``NewType`` assignments (see `_newtype_assignments`).
    """
    newtypes = newtypes or {}
    if isinstance(node, ast.Constant) and node.value is None:
        return type(None)
    if isinstance(node, ast.Name) and (assign := newtypes.get(node.id)):
        others = {k: v for k, v in newtypes.items() if k != node.id}
        supertype = _ast_type(typing.cast(ast.Call, assign.value).args[1], aliases, others)
        return None if supertype is None else typing.NewType(node.id, supertype)  # ty: ignore[invalid-newtype]
    if isinstance(node, ast.Name):
        known = _AST_TYPES | _TYPING_GENERICS if node.id in aliases else _AST_TYPES
        return known.get(aliases.get(node.id, node.id))
    if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
        module = aliases.get(node.value.id, node.value.id)
        return (_AST_TYPES | _TYPING_GENERICS).get(node.attr) if module in _TYPING_MODULES else None
    if isinstance(node, ast.Subscript) and _is_name(node.value, "Annotated", aliases):
        return _ast_type(
            node.slice.elts[0] if isinstance(node.slice, ast.Tuple) else node.slice,
            aliases,
            newtypes,
        )
    if (
        isinstance(node, ast.Subscript)
        and (origin := _ast_type(node.value, aliases, newtypes)) in _GENERICS
    ):
        args = node.slice.elts if isinstance(node.slice, ast.Tuple) else [node.slice]
        return types.GenericAlias(
            origin, tuple(_ast_type(a, aliases, newtypes) or typing.Any for a in args)
        )
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        members = [node.left, node.right]
    elif isinstance(node, ast.Subscript) and _is_name(node.value, "Optional", aliases):
        members = [node.slice, ast.Constant(None)]
    elif isinstance(node, ast.Subscript) and _is_name(node.value, "Union", aliases):
        members = list(node.slice.elts) if isinstance(node.slice, ast.Tuple) else [node.slice]
    else:
        return None
    resolved = [_ast_type(m, aliases, newtypes) for m in members]
    return None if None in resolved else functools.reduce(operator.or_, resolved)


def _annotation_type(annotation: str) -> typing.Any:
    """Parse an annotation string into a type as `_ast_type` does; None if it can't be resolved.

    Only for annotations without their module's imports; ``typing.X`` resolves.
    """
    try:
        return _ast_type(ast.parse(annotation, mode="eval").body, {n: n for n in _TYPING_GENERICS})
    except SyntaxError:
        return None


def _ast_key_types(
    nodes: list[ast.ClassDef], aliases: dict[str, str], newtypes: dict[str, ast.Assign]
) -> dict[str, typing.Any]:
    """Key types of a TypedDict lineage (bases first) from source, as by ``_ast_type``."""
    return {
        item.target.id: _ast_type(
            _unwrap_qualifiers_ast(item.annotation, aliases)[0], aliases, newtypes
        )
        for node in nodes
        for item in node.body
        if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name)
    }


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
    defaults_optional: bool = False,
) -> tuple[list[tuple[str, str]], frozenset[str]]:
    """Extract annotated fields as (name, type_str) pairs plus required names.

    Qualifiers are stripped from the type strings. This is the fallback
    when runtime resolution is not available. With ``defaults_optional``
    (dataclasses, Pydantic models) a field with a default is not required.
    """
    fields: list[tuple[str, str]] = []
    required: set[str] = set()
    for item in node.body:
        if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
            annotation, is_required = _unwrap_qualifiers_ast(item.annotation, aliases or {})
            fields.append((item.target.id, _normalize_type_str(ast.unparse(annotation))))
            if defaults_optional:
                if not _has_default(item.value):
                    required.add(item.target.id)
            elif total if is_required is None else is_required:
                required.add(item.target.id)
    return fields, frozenset(required)


def _has_default(value: ast.expr | None) -> bool:
    """Whether a dataclass / Pydantic field assigned ``value`` has a default.

    Example:
        >>> [_has_default(ast.parse(s, mode="eval").body) for s in ("1", "field(default_factory=list)", "Field(...)", "field()", "Field(0)")]
        [True, True, False, False, True]
    """
    if value is None:
        return False
    if isinstance(value, ast.Call) and _get_name(value.func) in ("field", "Field"):
        defaults = {"default", "default_factory", "factory"}
        if any(k.arg in defaults for k in value.keywords):
            return True
        first = value.args[0] if value.args else None
        return first is not None and not (isinstance(first, ast.Constant) and first.value is ...)
    return True


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
