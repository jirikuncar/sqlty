"""Subtype and consistency checks between resolved types."""

from __future__ import annotations

import datetime
import decimal
import functools
import operator
import types
import typing

if typing.TYPE_CHECKING:
    pass


_QUALIFIERS = frozenset({"ReadOnly", "Required", "NotRequired"})


def _unwrap_qualifiers(tp: typing.Any) -> typing.Any:
    """Strip ``ReadOnly`` / ``Required`` / ``NotRequired`` wrappers."""
    while typing.get_origin(tp) in (typing.ReadOnly, typing.Required, typing.NotRequired):
        tp = typing.get_args(tp)[0]
    return tp


_NEVER = (typing.Never, typing.NoReturn)


_PROMOTED_FROM: dict[type, tuple[type, ...]] = {float: (int,), complex: (int, float)}


# Pydantic lax mode; https://docs.pydantic.dev/latest/concepts/conversion_table/
_LAX_FROM: dict[type, tuple[type, ...]] = _PROMOTED_FROM | {
    int: (decimal.Decimal, float),
    decimal.Decimal: (int, float),
    bool: (int,),
    datetime.datetime: (datetime.date,),
}


def _union_members(tp: typing.Any) -> list[typing.Any]:
    if typing.get_origin(tp) in (typing.Union, types.UnionType):
        return list(typing.get_args(tp))
    return [tp]


def _is_plain(tp: typing.Any) -> bool:
    return isinstance(tp, type) and not typing.get_args(tp)


def _is_subtype(
    src: typing.Any, dst: typing.Any, promotions: dict[type, tuple[type, ...]] | None = None
) -> bool | None:
    """Whether ``src`` is assignable to ``dst``; None when that can't be decided.

    Only plain classes and unions of them are judged, by subclassing plus
    ``promotions`` (default: ``int`` -> ``float`` -> ``complex``). Generics,
    literals, forward references and unknown (None) types are undecided.

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
        fits = _member_fits(member, targets, promotions or _PROMOTED_FROM)
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


def _member_fits(
    src: typing.Any, targets: list[typing.Any], promotions: dict[type, tuple[type, ...]]
) -> bool | None:
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
            if issubclass(src, (dst, *promotions.get(dst, ()))):
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
