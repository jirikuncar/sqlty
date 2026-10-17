"""Whether a value a driver returned has the Python type sqlty inferred for its column."""

from __future__ import annotations

import datetime
import decimal
import ipaddress
import types
import typing
import uuid
from typing import Any

_NAMESPACE = {
    "datetime": datetime,
    "decimal": decimal,
    "ipaddress": ipaddress,
    "typing": typing,
    "uuid": uuid,
}


def matches(value: object, python_type: str) -> bool:
    """Whether ``value`` is an instance of ``python_type``, as sqlty spells it.

    Example:
        >>> matches([1, 2], "list[int]"), matches({"a": 1}, "dict[str, int | str]")
        (True, True)
        >>> matches(None, "int | None"), matches(1.5, "int")
        (True, False)
    """
    return _matches(value, eval(python_type, _NAMESPACE))


def _matches(value: object, tp: Any) -> bool:
    origin, args = typing.get_origin(tp), typing.get_args(tp)
    if tp is typing.Any:
        return True
    if tp is None or tp is type(None):
        return value is None
    if origin in (typing.Union, types.UnionType):
        return any(_matches(value, arg) for arg in args)
    if origin is typing.Literal:
        return value in args
    if origin is list:
        return isinstance(value, list) and all(_matches(item, args[0]) for item in value)
    if origin is dict:
        return isinstance(value, dict) and all(
            _matches(k, args[0]) and _matches(v, args[1]) for k, v in value.items()
        )
    if origin is tuple:
        if len(args) == 2 and args[1] is Ellipsis:
            return isinstance(value, tuple)
        return (
            isinstance(value, tuple)
            and len(value) == len(args)
            and all(_matches(item, arg) for item, arg in zip(value, args, strict=True))
        )
    if tp is float:
        # The numeric tower: a type checker accepts an `int` where `float` is annotated.
        return isinstance(value, (float, int)) and not isinstance(value, bool)
    if tp is int:
        return isinstance(value, int) and not isinstance(value, bool)
    return isinstance(value, tp)
