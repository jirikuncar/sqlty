"""Arguments passed to the parameters of a query, judged against the types the query gives them.

``arg_type`` reads the type of an argument where the source states it: a literal, a
variable annotated or assigned a class, a call of a known class. ``mismatches`` compares a
call's arguments with ``sqlty.core.analyze_params``; an argument of unknown type is never
reported.

Example:
    >>> import ast
    >>> from sqlty._source import ParsedSource
    >>> parsed = ParsedSource("f(1, 'a', None, [1.5], date(2020, 1, 1))\\nfrom datetime import date")
    >>> call = parsed.calls[0]
    >>> [arg_type(a, parsed, parsed.scopes_of(call)) for a in call.args]
    ['int', 'str', 'None', 'list[float]', 'datetime.date']
"""

from __future__ import annotations

import ast
from typing import TYPE_CHECKING, NamedTuple

if TYPE_CHECKING:
    from sqlty._source import ParsedSource, _Stack


class ParamArg(NamedTuple):
    """An argument of a query call: the parameter it binds (1-based position or name) and its type, if known."""

    key: int | str
    type: str | None


# Classes whose instances have a known type: builtins by name, the rest by the dotted name
# they are imported as.
_BUILTINS = frozenset({"int", "str", "float", "bool", "bytes"})
_IMPORTED = frozenset(
    {
        "datetime.date",
        "datetime.datetime",
        "datetime.time",
        "datetime.timedelta",
        "uuid.UUID",
        "decimal.Decimal",
    }
)
_SEQUENCES = frozenset({"list", "tuple", "set", "frozenset"})

# What a parameter of a given type takes. Any ``str`` is taken for any type: the server
# coerces text (psycopg sends it untyped), so only the other types are judged.
_ACCEPTS: dict[str, frozenset[str]] = {
    "int": frozenset({"int", "bool"}),
    "float": frozenset({"int", "float", "bool", "decimal.Decimal"}),
    "decimal.Decimal": frozenset({"int", "float", "bool", "decimal.Decimal"}),
    "str": frozenset({"str"}),
    "bool": frozenset({"bool", "int"}),
    "bytes": frozenset({"bytes"}),
    "datetime.date": frozenset({"datetime.date", "datetime.datetime"}),
    "datetime.datetime": frozenset({"datetime.date", "datetime.datetime"}),
    "datetime.time": frozenset({"datetime.time"}),
    "datetime.timedelta": frozenset({"datetime.timedelta"}),
    "uuid.UUID": frozenset({"uuid.UUID"}),
}


def arg_type(expr: ast.expr, parsed: ParsedSource, stack: _Stack) -> str | None:
    """The type ``expr`` has where the source says so, else ``None``.

    Spelled as the engine spells parameter types: ``int``, ``datetime.date``,
    ``list[int]``; ``None`` is ``"None"``. A list, tuple or set is ``list[T]`` when all its
    items have type ``T``, else ``list``.
    """
    match expr:
        case ast.Constant(value=None):
            return "None"
        case ast.Constant(value=value) if type(value) in (bool, int, float, str, bytes):
            return type(value).__name__
        case ast.UnaryOp(op=ast.USub() | ast.UAdd(), operand=ast.Constant(value=float())):
            return "float"
        case ast.UnaryOp(op=ast.USub() | ast.UAdd(), operand=ast.Constant(value=int())):
            return "int"
        case ast.JoinedStr():
            return "str"
        case ast.List(elts=items) | ast.Tuple(elts=items) | ast.Set(elts=items):
            types = {
                None if isinstance(item, ast.Starred) else arg_type(item, parsed, stack)
                for item in items
            }
            if len(types) == 1 and None not in types and "None" not in types:
                return f"list[{types.pop()}]"
            return "list"
        case ast.Name(id=name):
            return _class(parsed.type_of(stack, name), parsed)
        case ast.Call(func=func):
            return _class(_dotted(func), parsed)
    return None


def _dotted(node: ast.expr) -> str | None:
    parts: list[str] = []
    while isinstance(node, ast.Attribute):
        parts.append(node.attr)
        node = node.value
    return ".".join([node.id, *reversed(parts)]) if isinstance(node, ast.Name) else None


def _class(name: str | None, parsed: ParsedSource) -> str | None:
    """The type of an instance of the class spelled ``name`` in ``parsed``, when it is a known one."""
    if name is None:
        return None
    head, _, rest = name.partition(".")
    imported = parsed.imported_names.get(head)
    if imported is None:
        return name if name in _BUILTINS else "list" if name in _SEQUENCES else None
    full = f"{imported}.{rest}" if rest else imported
    return full if full in _IMPORTED else None


def mismatch(expected: str | None, nullable: bool | None, actual: str | None) -> str | None:
    """Why a value of type ``actual`` can't be passed for a parameter of type ``expected``, else ``None``.

    Example:
        >>> mismatch("int", None, "datetime.date")
        'expects int, got datetime.date'
        >>> mismatch("int", False, "None")
        'is not nullable, got None'
        >>> mismatch("list[int]", None, "list[float]")
        'expects list[int], got list[float]'
        >>> [mismatch("int", None, a) for a in ("bool", "str", None)]
        [None, None, None]
    """
    if actual is None:
        return None
    if actual == "None":
        return "is not nullable, got None" if nullable is False else None
    if expected is None or actual == "str":
        return None
    if expected.startswith("list["):
        if actual.startswith("list["):
            inner = mismatch(expected[5:-1], None, actual[5:-1])
            return None if inner is None else f"expects {expected}, got {actual}"
        return None if actual == "list" else f"expects {expected}, got {actual}"
    accepted = _ACCEPTS.get(expected)
    if accepted is None or actual in accepted:
        return None
    return f"expects {expected}, got {actual}"


def literal_params(
    arg: ast.expr | None, parsed: ParsedSource, stack: _Stack
) -> tuple[ParamArg, ...] | None:
    """The arguments a tuple or list literal, or a dict literal of string keys, passes; else ``None``."""
    if isinstance(arg, (ast.Tuple, ast.List)) and not any(
        isinstance(item, ast.Starred) for item in arg.elts
    ):
        return tuple(
            ParamArg(i, arg_type(item, parsed, stack)) for i, item in enumerate(arg.elts, 1)
        )
    if isinstance(arg, ast.Dict) and all(
        isinstance(key, ast.Constant) and isinstance(key.value, str) for key in arg.keys
    ):
        return tuple(
            ParamArg(str(key.value), arg_type(value, parsed, stack))
            for key, value in zip(arg.keys, arg.values, strict=True)
            if isinstance(key, ast.Constant)
        )
    return None
