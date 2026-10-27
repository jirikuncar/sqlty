"""AST-level extraction of fields for TypedDict, dataclass, NamedTuple and Pydantic classes."""

from __future__ import annotations

import ast
import collections
import datetime
import decimal
import functools
import operator
import types
import typing
import uuid

if typing.TYPE_CHECKING:
    pass

from sqlty._type_extractor.info import TypeInfo
from sqlty._type_extractor.stdlib import stdlib_class
from sqlty._type_extractor.subtype import _QUALIFIERS, _consistent, _is_subtype, _type_repr

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


def _module_types(tree: ast.Module, aliases: dict[str, str]) -> dict[str, ast.Assign | ast.expr]:
    """Module-level names a type can go through, read by `_ast_type`: aliases and ``NewType``s.

    An alias (``X = T``, ``X: TypeAlias = T``, ``type X = T``) maps to its value, a ``NewType``
    to its assignment. A name bound more than once, or a generic ``type X[T] = ...``, is left out.

    Example:
        >>> src = "Id = int\\ntype Name = str\\nU = NewType('U', Id)\\nx = f()\\nT = int\\nT = str"
        >>> sorted(_module_types(ast.parse(src), {"NewType": "NewType"}))
        ['Id', 'Name', 'U']
    """
    found: dict[str, ast.Assign | ast.expr] = {}
    bound: collections.Counter[str] = collections.Counter()
    for node in tree.body:
        match node:
            case ast.Assign(targets=[ast.Name(id=name)], value=value) if all(
                isinstance(n, _TYPE_EXPR_NODES) for n in ast.walk(value)
            ):
                found[name] = value
            case ast.AnnAssign(target=ast.Name(id=name), annotation=annotation, value=value) if (
                value is not None and _is_name(annotation, "TypeAlias", aliases)
            ):
                found[name] = value
            case ast.TypeAlias(name=ast.Name(id=name), type_params=[], value=value):
                found[name] = value
        bound.update(_bound_names(node))
    found |= _newtype_assignments(tree, aliases)
    return {name: value for name, value in found.items() if bound[name] == 1}


def _bound_names(node: ast.stmt) -> list[str]:
    """Names a module-level statement binds."""
    match node:
        case ast.FunctionDef(name=name) | ast.AsyncFunctionDef(name=name) | ast.ClassDef(name=name):
            return [name]
        case ast.Import(names=names) | ast.ImportFrom(names=names):
            return [(a.asname or a.name).partition(".")[0] for a in names]
    return [
        n.id for n in ast.walk(node) if isinstance(n, ast.Name) and isinstance(n.ctx, ast.Store)
    ]


def _classify_class(
    node: ast.ClassDef,
    aliases: dict[str, str] | None = None,
    typeddicts: typing.Container[str] = (),
    models: typing.Container[str] = (),
) -> str | None:
    """Classify a class as TypedDict, NamedTuple, dataclass, BaseModel or Protocol.

    Subclasses of the names in ``typeddicts`` are TypedDicts too, and of ``models`` models.
    """
    aliases = aliases or {}
    for base in node.bases:
        base_name = _get_name(base)
        base_name = aliases.get(base_name, base_name) if base_name else None
        if base_name == "TypedDict" or (isinstance(base, ast.Name) and base.id in typeddicts):
            return "TypedDict"
        if base_name == "NamedTuple":
            return "NamedTuple"
        if base_name == "BaseModel" or (isinstance(base, ast.Name) and base.id in models):
            return "BaseModel"
        if base_name == "Protocol":
            return "Protocol"

    for decorator in node.decorator_list:
        dec_name = _get_name(decorator)
        if dec_name == "dataclass":
            return "dataclass"

    return None


def _functional_classes(tree: ast.Module, aliases: dict[str, str]) -> list[ast.ClassDef]:
    """``R = TypedDict("R", {...})`` and ``N = NamedTuple("N", [(...), ...])`` at module level, as classes.

    Each class keeps the position of its assignment, whose source builds it at runtime.

    Example:
        >>> tree = ast.parse('R = TypedDict("R", {"a": int}, total=False)')
        >>> [ast.unparse(c) for c in _functional_classes(tree, {"TypedDict": "TypedDict"})]
        ['class R(TypedDict, total=False):\\n    a: int']
    """
    classes: list[ast.ClassDef] = []
    for stmt in tree.body:
        if not (
            isinstance(stmt, ast.Assign)
            and len(stmt.targets) == 1
            and isinstance(target := stmt.targets[0], ast.Name)
            and isinstance(call := stmt.value, ast.Call)
            and len(call.args) == 2
        ):
            continue
        name = _get_name(call.func) or ""
        kind, spec = aliases.get(name, name), call.args[1]
        if kind == "TypedDict" and isinstance(spec, ast.Dict):
            pairs = list(zip(spec.keys, spec.values, strict=True))
        elif kind == "NamedTuple" and isinstance(spec, (ast.List, ast.Tuple)):
            pairs = [
                (item.elts[0], item.elts[1])
                for item in spec.elts
                if isinstance(item, ast.Tuple) and len(item.elts) == 2
            ]
            if len(pairs) != len(spec.elts):
                continue
        else:
            continue
        fields = [
            (key.value, value)
            for key, value in pairs
            if isinstance(key, ast.Constant) and isinstance(key.value, str)
        ]
        if len(fields) != len(pairs):
            continue
        body: list[ast.stmt] = [
            ast.copy_location(
                ast.AnnAssign(target=ast.Name(field, ast.Store()), annotation=value, simple=1),
                value,
            )
            for field, value in fields
        ]
        node = ast.ClassDef(
            name=target.id,
            bases=[call.func],
            keywords=call.keywords,
            body=body or [ast.Pass()],
            decorator_list=[],
            type_params=[],
        )
        classes.append(ast.copy_location(node, stmt))
    return classes


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


_STRICT_TYPES: dict[str, type] = {
    "StrictInt": int,
    "StrictFloat": float,
    "StrictBool": bool,
    "StrictStr": str,
    "StrictBytes": bytes,
}


_STRICT_NAMES = {
    f"{m}.{n}": t for m in ("pydantic", "pydantic.types") for n, t in _STRICT_TYPES.items()
}


_IMPORTED_TYPES: dict[str, typing.Any] = {
    "decimal.Decimal": decimal.Decimal,
    "datetime.date": datetime.date,
    "datetime.datetime": datetime.datetime,
    "datetime.time": datetime.time,
    "datetime.timedelta": datetime.timedelta,
    "uuid.UUID": uuid.UUID,
    **_STRICT_NAMES,
}


class TypeNames(typing.Protocol):
    """What `_ast_type` reads a name through: the value of an alias, or a type."""

    def get(self, name: str, /) -> ast.expr | typing.Any: ...


class _Hiding:
    """``names`` without ``hidden``, so an alias that refers to itself is unknown."""

    def __init__(self, names: TypeNames, hidden: str) -> None:
        self.names = names
        self.hidden = hidden

    def get(self, name: str, /) -> ast.expr | typing.Any:
        return None if name == self.hidden else self.names.get(name)


def _ast_type(
    node: ast.expr,
    aliases: dict[str, str],
    newtypes: TypeNames | None = None,
    imports: dict[str, str] | None = None,
) -> typing.Any:
    """Builtin scalars and generics, ``Any``, ``Never``, NewTypes and their unions from source; None if unknown.

    ``aliases`` maps local names to ``typing`` names (see ``ParsedSource.typing_aliases``), so
    ``A`` for ``from typing import Any as A`` and ``t.Any`` for
    ``import typing as t`` resolve; ``Annotated`` metadata is dropped.
    ``typing`` generic aliases (``List``, ``Dict``, ``Set``, ``FrozenSet``,
    ``Tuple``) resolve to their builtins only when imported from ``typing``
    or ``typing_extensions``.
    ``newtypes`` maps names (``X`` or ``mod.X``) to the values of aliases, or to types
    already resolved, such as ``NewType``s (see ``symbols.ModuleTypes``). A string annotation is parsed, never evaluated, and resolved the same way.
    With ``imports`` (see ``ParsedSource.imported_names``), the ``decimal``,
    ``datetime`` and ``uuid`` types and Pydantic's ``Strict*`` aliases resolve too.
    """
    newtypes = newtypes or {}
    if isinstance(node, ast.Constant) and node.value is None:
        return type(None)
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        try:
            parsed = ast.parse(node.value.strip(), mode="eval").body
        except SyntaxError:
            return None
        return _ast_type(parsed, aliases, newtypes, imports)
    if imports and (qualname := _qualname(node, imports)):
        if tp := _IMPORTED_TYPES.get(qualname) or stdlib_class(qualname):
            return tp
    if isinstance(node, (ast.Name, ast.Attribute)) and (
        (found := newtypes.get(_qualname(node, {}) or "")) is not None
        and not isinstance(found, ast.AST)
    ):
        return found
    if isinstance(node, ast.Name) and (alias := newtypes.get(node.id)) is not None:
        return _ast_type(alias, aliases, _Hiding(newtypes, node.id), imports)
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
            imports,
        )
    if isinstance(node, ast.Subscript) and _is_name(node.value, "Literal", aliases):
        items = node.slice.elts if isinstance(node.slice, ast.Tuple) else [node.slice]
        if all(isinstance(i, ast.Constant) and isinstance(i.value, (str, int)) for i in items):
            literal: typing.Any = typing.Literal
            return literal[tuple(typing.cast(ast.Constant, i).value for i in items)]
        return None
    if (
        isinstance(node, ast.Subscript)
        and (origin := _ast_type(node.value, aliases, newtypes, imports)) in _GENERICS
    ):
        args = node.slice.elts if isinstance(node.slice, ast.Tuple) else [node.slice]
        return types.GenericAlias(
            origin, tuple(_ast_type(a, aliases, newtypes, imports) or typing.Any for a in args)
        )
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        members = [node.left, node.right]
    elif isinstance(node, ast.Subscript) and _is_name(node.value, "Optional", aliases):
        members = [node.slice, ast.Constant(None)]
    elif isinstance(node, ast.Subscript) and _is_name(node.value, "Union", aliases):
        members = list(node.slice.elts) if isinstance(node.slice, ast.Tuple) else [node.slice]
    else:
        return None
    resolved = [_ast_type(m, aliases, newtypes, imports) for m in members]
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
    nodes: list[ast.ClassDef],
    aliases: dict[str, str],
    newtypes: TypeNames,
    imports: dict[str, str] | None = None,
) -> dict[str, typing.Any]:
    """Key types of a TypedDict lineage (bases first) from source, as by ``_ast_type``."""
    return {
        item.target.id: _ast_type(
            _unwrap_qualifiers_ast(item.annotation, aliases)[0], aliases, newtypes, imports
        )
        for node in nodes
        for item in node.body
        if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name)
    }


def _ast_annotations(nodes: list[ast.ClassDef], aliases: dict[str, str]) -> dict[str, ast.expr]:
    """Field annotations of a class lineage (bases first), qualifiers stripped, ``| None`` kept."""
    return {
        item.target.id: _unwrap_qualifiers_ast(item.annotation, aliases)[0]
        for node in nodes
        for item in node.body
        if isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name)
    }


def _is_name(node: ast.expr, name: str, aliases: dict[str, str]) -> bool:
    return (n := _get_name(node)) is not None and aliases.get(n, n) == name


def _qualname(node: ast.expr, imports: dict[str, str]) -> str | None:
    """The dotted name ``node`` spells, its first part resolved through ``imports``.

    Example:
        >>> _qualname(ast.parse("dt.date", mode="eval").body, {"dt": "datetime"})
        'datetime.date'
    """
    if isinstance(node, ast.Name):
        return imports.get(node.id, node.id)
    if isinstance(node, ast.Attribute) and (base := _qualname(node.value, imports)):
        return f"{base}.{node.attr}"
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


def _extract_fields_from_ast(
    node: ast.ClassDef,
    aliases: dict[str, str] | None = None,
    *,
    total: bool = True,
    defaults_optional: bool = False,
    kind: str = "",
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
            if not _takes_column(item, item.target.id, kind, aliases or {}):
                continue
            annotation, is_required = _unwrap_qualifiers_ast(item.annotation, aliases or {})
            fields.append((item.target.id, _normalize_type_str(ast.unparse(annotation))))
            if _metadata_has_default(item.annotation, aliases or {}):
                continue
            if defaults_optional:
                if not _has_default(item.value):
                    required.add(item.target.id)
            elif total if is_required is None else is_required:
                required.add(item.target.id)
    return fields, frozenset(required)


def _takes_column(item: ast.AnnAssign, name: str, kind: str, aliases: dict[str, str]) -> bool:
    """Whether the class-body annotation ``item`` is a constructor argument, which a column fills.

    A ``ClassVar`` is not, nor are a dataclass's ``KW_ONLY`` marker and ``field(init=False)``
    fields, or a Pydantic model's private ``_attributes``.

    Example:
        >>> items = ast.parse("a: ClassVar[int]\\n_: KW_ONLY\\nb: int = field(init=False)\\nc: int").body
        >>> [_takes_column(i, ast.unparse(i.target), "dataclass", {}) for i in items]
        [False, False, False, True]
    """
    annotation = item.annotation
    if _is_name(
        annotation.value if isinstance(annotation, ast.Subscript) else annotation,
        "ClassVar",
        aliases,
    ):
        return False
    if kind == "BaseModel":
        return not name.startswith("_")
    if kind != "dataclass":
        return True
    if _get_name(annotation) == "KW_ONLY":
        return False
    value = item.value
    return not (
        isinstance(value, ast.Call)
        and _get_name(value.func) in ("field", "Field")
        and any(
            k.arg == "init" and isinstance(k.value, ast.Constant) and k.value.value is False
            for k in value.keywords
        )
    )


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


def _field_calls(annotation: ast.expr, aliases: dict[str, str]) -> list[ast.Call]:
    """``Field(...)`` / ``field(...)`` calls in the ``Annotated`` metadata of ``annotation``.

    Example:
        >>> a = ast.parse("NotRequired[Annotated[list[str], Field(default_factory=list), 1]]", mode="eval").body
        >>> [ast.unparse(c) for c in _field_calls(a, {})]
        ['Field(default_factory=list)']
    """
    calls: list[ast.Call] = []
    while isinstance(annotation, ast.Subscript):
        name = _get_name(annotation.value)
        name = aliases.get(name, name) if name else None
        if name == "Annotated" and isinstance(annotation.slice, ast.Tuple):
            elts = annotation.slice.elts
            calls += [
                m
                for m in elts[1:]
                if isinstance(m, ast.Call) and _get_name(m.func) in ("field", "Field")
            ]
            annotation = elts[0]
        elif name in _QUALIFIERS:
            annotation = annotation.slice
        else:
            break
    return calls


def _metadata_has_default(annotation: ast.expr, aliases: dict[str, str]) -> bool:
    """Whether ``Annotated`` metadata of ``annotation`` declares a default (any row kind)."""
    return any(_has_default(call) for call in _field_calls(annotation, aliases))


def _metadata_defaults(node: ast.ClassDef, aliases: dict[str, str]) -> list[str]:
    """Names of the fields of ``node`` whose ``Annotated`` metadata declares a default."""
    return [
        item.target.id
        for item in node.body
        if isinstance(item, ast.AnnAssign)
        and isinstance(item.target, ast.Name)
        and _metadata_has_default(item.annotation, aliases)
    ]


def _config_options(node: ast.ClassDef, config: ast.expr | None = None) -> dict[str, ast.expr]:
    """Pydantic config of ``node``: ``model_config`` / ``__pydantic_config__ = ConfigDict(...)`` / ``{...}``, ``class Config`` or ``config``."""
    options: dict[str, ast.expr] = {}

    def read(value: ast.expr | None) -> None:
        if isinstance(value, ast.Call):
            options.update({k.arg: k.value for k in value.keywords if k.arg})
        elif isinstance(value, ast.Dict):
            options.update(
                {
                    k.value: v
                    for k, v in zip(value.keys, value.values, strict=True)
                    if isinstance(k, ast.Constant) and isinstance(k.value, str)
                }
            )

    options.update({k.arg: k.value for k in node.keywords if k.arg})
    read(config)
    names = ("model_config", "__pydantic_config__")
    for item in node.body:
        if (
            isinstance(item, ast.Assign)
            and any(isinstance(t, ast.Name) and t.id in names for t in item.targets)
            or (
                isinstance(item, ast.AnnAssign)
                and isinstance(item.target, ast.Name)
                and item.target.id in names
            )
        ):
            read(item.value)
        elif isinstance(item, ast.ClassDef) and item.name == "Config":
            for stmt in item.body:
                if isinstance(stmt, ast.Assign):
                    options.update(
                        {t.id: stmt.value for t in stmt.targets if isinstance(t, ast.Name)}
                    )
    return options


def _is_true(node: ast.expr | None) -> bool:
    return isinstance(node, ast.Constant) and node.value is True


def _pydantic_config(
    node: ast.ClassDef, kind: str, imports: dict[str, str]
) -> dict[str, ast.expr] | None:
    """Config of a Pydantic model or ``pydantic.dataclasses.dataclass``; None for other rows."""
    if kind == "BaseModel":
        return _config_options(node)
    for decorator in node.decorator_list:
        call = decorator if isinstance(decorator, ast.Call) else None
        if _qualname(call.func if call else decorator, imports) == "pydantic.dataclasses.dataclass":
            keywords = call.keywords if call else []
            return _config_options(
                node, next((k.value for k in keywords if k.arg == "config"), None)
            )
    return None


def _strict(call: ast.Call) -> bool | None:
    """``strict`` set by ``Field(strict=...)`` or ``Strict(...)``; None if unset."""
    name = _get_name(call.func)
    if name not in ("Field", "Strict"):
        return None
    value = next((k.value for k in call.keywords if k.arg == "strict"), None)
    if name == "Strict":
        value = call.args[0] if call.args else value or ast.Constant(True)
    return (
        value.value if isinstance(value, ast.Constant) and isinstance(value.value, bool) else None
    )


def _lax_fields(
    node: ast.ClassDef,
    config: dict[str, ast.expr],
    aliases: dict[str, str],
    imports: dict[str, str],
) -> frozenset[str]:
    """Fields of a Pydantic row that are not strict, by ``config``, ``Field``, ``Strict()`` or ``Strict*`` types."""
    model = _is_true(config.get("strict"))
    lax: set[str] = set()
    for item in node.body:
        if not (isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name)):
            continue
        pending, members = [item.annotation], []
        metadata: list[ast.expr | None] = []
        while pending:
            annotation = pending.pop()
            name = _get_name(annotation.value) if isinstance(annotation, ast.Subscript) else None
            name = aliases.get(name, name) if name else None
            if isinstance(annotation, ast.BinOp) and isinstance(annotation.op, ast.BitOr):
                pending += [annotation.left, annotation.right]
            elif not isinstance(annotation, ast.Subscript):
                members.append(annotation)
            elif name == "Annotated" and isinstance(annotation.slice, ast.Tuple):
                metadata[:0] = annotation.slice.elts[1:]
                pending.append(annotation.slice.elts[0])
            elif name in (*_QUALIFIERS, "Optional"):
                pending.append(annotation.slice)
            elif name == "Union" and isinstance(annotation.slice, ast.Tuple):
                pending += annotation.slice.elts
            else:
                members.append(annotation)
        metadata.append(item.value)
        found = [s for m in metadata if isinstance(m, ast.Call) and (s := _strict(m)) is not None]
        typed = any(_qualname(m, imports) in _STRICT_NAMES for m in members)
        if not (typed or (found[-1] if found else model)):
            lax.add(item.target.id)
    return frozenset(lax)


def _allows_extra(
    node: ast.ClassDef, options: dict[str, ast.expr], models: typing.Container[str] = ()
) -> bool:
    """Whether a Pydantic model keeps undeclared columns: ``extra="allow"``, or a config not read.

    ``extra="ignore"`` (the default) drops them and ``extra="forbid"`` rejects them. A config
    sqlty cannot read from source (a ``model_config`` that is not a literal, ``**`` options, a
    base other than ``BaseModel``, ``Generic`` and the ``models`` read from source) may allow them.

    Example:
        >>> cls = lambda src: ast.parse(src).body[0]
        >>> _allows_extra(cls("class A(BaseModel): x: int"), {})
        False
        >>> _allows_extra(cls("class A(BaseModel): model_config = CONFIG"), {})
        True
    """
    if (extra := options.get("extra")) is not None:
        value = (
            extra.value
            if isinstance(extra, ast.Constant)
            else extra.attr
            if isinstance(extra, ast.Attribute)
            else None
        )
        return value not in ("ignore", "forbid")
    if any(k.arg is None for k in node.keywords):
        return True
    for base in node.bases:
        root = base.value if isinstance(base, ast.Subscript) else base
        if _get_name(root) not in ("BaseModel", "Generic") and not (
            isinstance(root, ast.Name) and root.id in models
        ):
            return True
    names = ("model_config", "__pydantic_config__")
    for item in node.body:
        targets = (
            item.targets
            if isinstance(item, ast.Assign)
            else [item.target]
            if isinstance(item, ast.AnnAssign)
            else []
        )
        if not any(isinstance(t, ast.Name) and t.id in names for t in targets):
            continue
        config = typing.cast(ast.Assign | ast.AnnAssign, item).value
        if isinstance(config, ast.Call):
            if any(k.arg is None for k in config.keywords) or config.args:
                return True
        elif isinstance(config, ast.Dict):
            if None in config.keys:
                return True
        else:
            return True
    return False


def _set_pydantic_names(info: TypeInfo, nodes: list[ast.ClassDef], aliases: dict[str, str]) -> None:
    """Read validation aliases and the ``extra`` setting from the class source.

    A field is matched by its ``alias`` / ``validation_alias`` string (either
    that or its name with ``populate_by_name=True``). ``AliasChoices``,
    ``AliasPath``, non-literal aliases and ``alias_generator`` leave the
    field's names unknown (``opaque``).
    """
    options = {key: value for node in nodes for key, value in _config_options(node).items()}
    models = {node.name for node in nodes}
    info.allows_extra = any(_allows_extra(node, options, models) for node in nodes)
    by_name = _is_true(options.get("populate_by_name")) or _is_true(options.get("validate_by_name"))
    generated = "alias_generator" in options and not (
        isinstance(options["alias_generator"], ast.Constant)
        and options["alias_generator"].value is None
    )
    columns: dict[str, str] = {}
    opaque: set[str] = set()
    for item in (item for node in nodes for item in node.body):
        if not (isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name)):
            continue
        name = item.target.id
        if not _takes_column(item, name, "BaseModel", aliases):
            continue
        calls = _field_calls(item.annotation, {})
        if isinstance(item.value, ast.Call) and _get_name(item.value.func) == "Field":
            calls.append(item.value)
        keywords = {k.arg: k.value for call in calls for k in call.keywords if k.arg}
        spec = keywords.get("validation_alias", keywords.get("alias"))
        if spec is None:
            if generated:
                opaque.add(name)
            else:
                columns[name] = name
        elif isinstance(spec, ast.Constant) and isinstance(spec.value, str):
            columns[spec.value] = name
            if by_name:
                columns.setdefault(name, name)
        else:
            opaque.add(name)
    info.columns = columns
    info.opaque = frozenset(opaque)


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
