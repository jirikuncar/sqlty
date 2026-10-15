"""Check the JSON a query builds against the annotation of the row field that receives it.

The engine reports the shape of a ``jsonb_build_object()``, ``jsonb_agg()`` or ``to_jsonb()``
column as JSON text (see ``rust/crates/sqlty-sql/src/json.rs``); drivers decode the value into
dicts, lists and scalars. `json_problems` compares that shape with the annotation, following
nested TypedDicts, ``list[...]`` and ``dict[str, ...]``, and lists what cannot hold it.

Only what is decidable is reported: ``Any``, ``object``, classes sqlty cannot see into
(imports, Pydantic models) and annotations it cannot read accept anything.
"""

from __future__ import annotations

import ast
import json
import typing

if typing.TYPE_CHECKING:
    from collections.abc import Mapping

    from sqlty._type_extractor import TypeInfo

# Annotation heads a JSON scalar decodes into (bool is an int, an int a float; a number
# is an int when integral, else a float, and ``Decimal`` loads it exactly).
_SCALAR_HEADS = {
    "boolean": {"bool", "int", "float"},
    "integer": {"int", "float"},
    "number": {"float", "int", "Decimal"},
    "string": {"str"},
}
_SCALAR_NAMES = {
    "boolean": "a boolean",
    "integer": "an integer",
    "number": "a number",
    "string": "a string",
}
_LISTS = {"list", "List", "Sequence", "MutableSequence", "Collection", "Iterable"}
_MAPPINGS = {"dict", "Dict", "Mapping", "MutableMapping"}
# Classes that can never hold a JSON value of another kind.
_BUILTINS = {"bool", "int", "float", "str", "bytes", "tuple", "Tuple", "set", "frozenset"}
_OPEN = {"Any", "object"}
_MAX_PROBLEMS = 3
_MAX_DEPTH = 32


class _Context(typing.NamedTuple):
    defs: Mapping[str, TypeInfo]
    aliases: Mapping[str, str]


def json_problems(
    shape: str,
    annotation: ast.expr,
    defs: Mapping[str, TypeInfo],
    aliases: Mapping[str, str],
    name: str = "",
) -> list[str]:
    """What keeps ``annotation`` from holding the JSON of ``shape``, as sentences; empty if it fits.

    ``shape`` is the JSON text the engine reports for a column; ``defs`` are the user types
    of the file (see ``extract_type_definitions``) and ``aliases`` its typing names. Each
    sentence starts with the path of the value below ``name``: ``items[].price is int but ...``.

    Example:
        >>> shape = '{"array":{"shape":"integer","nullable":true}}'
        >>> annotation = ast.parse("list[int]", mode="eval").body
        >>> json_problems(shape, annotation, {}, {})
        ['[] can be null but is declared int']
        >>> json_problems(shape, ast.parse("dict[str, int]", mode="eval").body, {}, {})
        ['is dict[str, int] but the query builds an array']
    """
    out: list[str] = []
    value = {"shape": json.loads(shape), "nullable": False}
    _check(value, annotation, name, _Context(defs, aliases), out, 0)
    return out


def render(shape: str) -> str:
    """The JSON shape the engine reports, as readable text.

    Objects are ``{key: shape}`` (``key?`` when it may be missing), arrays ``[shape]``,
    maps ``{[key]: shape}``, and a value that can be ``null`` ends in ``| null``.

    Example:
        >>> render('{"array":{"shape":{"object":[{"name":"id","value":{"shape":"integer","nullable":false},"optional":true}]},"nullable":true}}')
        '[{id?: integer} | null]'
    """
    return _render(json.loads(shape))


def _render(shape: typing.Any) -> str:
    if isinstance(shape, str):
        return shape
    kind, body = next(iter(shape.items()))
    if kind == "array":
        return f"[{_render_value(body)}]"
    if kind == "map":
        return f"{{[key]: {_render_value(body)}}}"
    fields = ", ".join(
        f"{f['name']}{'?' if f['optional'] else ''}: {_render_value(f['value'])}" for f in body
    )
    return f"{{{fields}}}"


def _render_value(value: Mapping[str, typing.Any]) -> str:
    text = _render(value["shape"])
    return f"{text} | null" if value["nullable"] else text


def summarize(problems: list[str]) -> str:
    """The first few problems on one line, the rest counted."""
    shown = "; ".join(problems[:_MAX_PROBLEMS])
    extra = len(problems) - _MAX_PROBLEMS
    return f"{shown}; and {extra} more" if extra > 0 else shown


def _head(node: ast.expr, ctx: _Context) -> str | None:
    """The name a (possibly subscripted) annotation is, ``typing`` aliases resolved."""
    if isinstance(node, ast.Subscript):
        node = node.value
    if isinstance(node, ast.Name):
        return ctx.aliases.get(node.id, node.id)
    if isinstance(node, ast.Attribute):
        return node.attr
    return None


def _is_none(node: ast.expr) -> bool:
    return isinstance(node, ast.Constant) and node.value is None


def _members(node: ast.expr, ctx: _Context, depth: int = 0) -> list[ast.expr]:
    """The alternatives of a union, ``Optional`` and ``Annotated`` looked through."""
    if depth > _MAX_DEPTH:
        return [node]
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        try:
            return _members(ast.parse(node.value, mode="eval").body, ctx, depth + 1)
        except SyntaxError, ValueError:
            return [node]
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        return _members(node.left, ctx, depth + 1) + _members(node.right, ctx, depth + 1)
    if isinstance(node, ast.Subscript):
        head = _head(node, ctx)
        args = list(node.slice.elts) if isinstance(node.slice, ast.Tuple) else [node.slice]
        if head == "Optional":
            return [*_members(args[0], ctx, depth + 1), ast.Constant(None)]
        if head == "Union":
            return [m for arg in args for m in _members(arg, ctx, depth + 1)]
        if head == "Annotated":
            return _members(args[0], ctx, depth + 1)
    return [node]


def _admits_none(members: list[ast.expr], ctx: _Context) -> bool:
    for member in members:
        if _is_none(member) or _head(member, ctx) in _OPEN:
            return True
        if _head(member, ctx) == "Literal" and isinstance(member, ast.Subscript):
            args = member.slice.elts if isinstance(member.slice, ast.Tuple) else [member.slice]
            if any(_is_none(arg) for arg in args):
                return True
    return False


def _describe(shape: typing.Any) -> str:
    if isinstance(shape, str):
        return _SCALAR_NAMES.get(shape, shape)
    return {"array": "an array", "map": "a map", "object": "an object"}[next(iter(shape))]


def _at(path: str, text: str) -> str:
    return f"{path} {text}" if path else text


def _check(
    value: Mapping[str, typing.Any],
    node: ast.expr,
    path: str,
    ctx: _Context,
    out: list[str],
    depth: int,
) -> None:
    if depth > _MAX_DEPTH:
        return
    members = _members(node, ctx)
    if value["nullable"] and not _admits_none(members, ctx):
        out.append(_at(path, f"can be null but is declared {ast.unparse(node)}"))
    shape = value["shape"]
    if shape in ("any", "never", "null"):
        return
    candidates = [m for m in members if not _is_none(m)]
    if not candidates:
        out.append(_at(path, f"is None but the query builds {_describe(shape)}"))
        return
    failures: list[list[str]] = []
    for member in candidates:
        found: list[str] = []
        _check_member(shape, member, path, ctx, found, depth)
        if not found:
            return
        failures.append(found)
    out.extend(failures[0])


def _mismatch(shape: typing.Any, member: ast.expr, path: str, out: list[str]) -> None:
    out.append(_at(path, f"is {ast.unparse(member)} but the query builds {_describe(shape)}"))


def _check_member(
    shape: typing.Any,
    member: ast.expr,
    path: str,
    ctx: _Context,
    out: list[str],
    depth: int,
) -> None:
    head = _head(member, ctx)
    if head is None or head in _OPEN:
        return
    info = ctx.defs.get(head) if isinstance(member, ast.Name) else None
    row_type = info if info is not None and info.kind in ("TypedDict", "BaseModel") else None
    if isinstance(shape, str):
        if head in _BUILTINS | _LISTS | _MAPPINGS or row_type:
            if head not in _SCALAR_HEADS[shape]:
                _mismatch(shape, member, path, out)
        return
    kind = next(iter(shape))
    if kind == "array":
        if head in _LISTS:
            if isinstance(member, ast.Subscript):
                _check(shape["array"], member.slice, f"{path}[]", ctx, out, depth + 1)
        elif head in _BUILTINS | _MAPPINGS or row_type:
            _mismatch(shape, member, path, out)
    elif kind == "map":
        if head in _MAPPINGS:
            if parts := _mapping_parts(member, path, ctx, out):
                _check(shape["map"], parts, f"{path}[key]", ctx, out, depth + 1)
        elif head in _BUILTINS | _LISTS or row_type:
            _mismatch(shape, member, path, out)
    elif head in _MAPPINGS:
        if parts := _mapping_parts(member, path, ctx, out):
            for field in shape["object"]:
                _check(field["value"], parts, _join(path, field["name"]), ctx, out, depth + 1)
    elif row_type is not None:
        _check_row_type(shape["object"], row_type, path, ctx, out, depth)
    elif head in _BUILTINS | _LISTS:
        _mismatch(shape, member, path, out)


def _mapping_parts(member: ast.expr, path: str, ctx: _Context, out: list[str]) -> ast.expr | None:
    """The value annotation of ``dict[K, V]``, reporting a key type that is not ``str``."""
    if not isinstance(member, ast.Subscript) or not isinstance(member.slice, ast.Tuple):
        return None
    key, item = member.slice.elts[0], member.slice.elts[-1]
    if (head := _head(key, ctx)) in _BUILTINS and head != "str":
        out.append(_at(path, f"has keys of {ast.unparse(key)} but JSON object keys are strings"))
    return item


def _join(path: str, name: str) -> str:
    return f"{path}.{name}" if path else name


def _check_row_type(
    fields: list[Mapping[str, typing.Any]],
    info: TypeInfo,
    path: str,
    ctx: _Context,
    out: list[str],
    depth: int,
) -> None:
    inner = _Context(ctx.defs, info.aliases or ctx.aliases)
    built = {field["name"]: field for field in fields}
    # A Pydantic model reads keys by alias; its field types are not checked here.
    model = info.kind == "BaseModel"
    admits_extra = info.extra_items or info.allows_extra or info.opaque
    for name, field in built.items():
        where = _join(path, name)
        key = info.field_for(name)
        if key is None:
            if not admits_extra:
                out.append(
                    _at(path or "the object", f"has key '{name}' that {info.name} does not declare")
                )
            continue
        if field["optional"] and key in info.required:
            out.append(_at(where, f"is required by {info.name} but the query may omit it"))
        if not model and (annotation := info.annotations.get(key)) is not None:
            _check(field["value"], annotation, where, inner, out, depth + 1)
    for key, _ in info.fields:
        names = info.column_names(key)
        if key in info.required and key not in info.opaque and not built.keys() & set(names):
            out.append(
                _at(
                    _join(path, names[0]),
                    f"is required by {info.name} but the query does not build it",
                )
            )
