"""Which diagnostics of a schema change only say an annotation is wider than it needs to be.

Code annotated ``str | None`` reads the ``str`` rows of a ``NOT NULL`` column fine, so
``sqlty breaking`` does not count a ``stale-type`` whose annotation admits the inferred
row type, nor ``redundant-optional`` and ``redundant-literal``.

Example:
    >>> widens({"rule": "stale-type", "message": "SQL[tuple[str | None]] should be SQL[tuple[str]]"})
    True
    >>> widens({"rule": "stale-type", "message": "SQL[tuple[str]] should be SQL[tuple[str | None]]"})
    False
"""

from __future__ import annotations

import re

from sqlty._lint import Diagnostic

_STALE = re.compile(
    r"(?P<wrapper>SQL|SQLTemplate)\[(?P<annotated>.*)\] should be (?P=wrapper)\[(?P<inferred>.*)\]"
)
_WIDER = frozenset({"redundant-optional", "redundant-literal"})


def widens(diagnostic: Diagnostic) -> bool:
    """Whether ``diagnostic`` only reports an annotation that admits every row the query returns."""
    if diagnostic["rule"] in _WIDER:
        return True
    match = _STALE.fullmatch(diagnostic["message"]) if diagnostic["rule"] == "stale-type" else None
    if match is None:
        return False
    annotated, inferred = _items(match["annotated"]), _items(match["inferred"])
    return (
        annotated is not None
        and inferred is not None
        and len(annotated) == len(inferred)
        and all(_admits(a, i) for a, i in zip(annotated, inferred, strict=True))
    )


def _admits(annotated: str, inferred: str) -> bool:
    members = set(_split(annotated, "|"))
    if members & {"typing.Any", "object", "Any"}:
        return True
    return all(
        member in members or _literal_base(member) in members for member in _split(inferred, "|")
    )


def _literal_base(member: str) -> str | None:
    """The class of a ``Literal[...]``'s values: ``str`` for ``Literal['a', 'b']``."""
    match = re.fullmatch(r"(?:typing\.)?Literal\[(.*)\]", member)
    if match is None:
        return None
    values = _split(match[1], ",")
    if all(value[:1] in "'\"" for value in values):
        return "str"
    if all(value in ("True", "False") for value in values):
        return "bool"
    if all(re.fullmatch(r"-?\d+", value) for value in values):
        return "int"
    return None


def _items(row: str) -> list[str] | None:
    """The item types of ``tuple[...]``; ``None`` for another row type."""
    if not (row.startswith("tuple[") and row.endswith("]")):
        return None
    return _split(row[len("tuple[") : -1], ",")


def _split(text: str, separator: str) -> list[str]:
    """``text`` split at ``separator`` outside brackets and quotes."""
    parts, depth, quote, start = [], 0, "", 0
    for i, char in enumerate(text):
        if quote:
            quote = "" if char == quote else quote
        elif char in "'\"":
            quote = char
        elif char == "[":
            depth += 1
        elif char == "]":
            depth -= 1
        elif char == separator and depth == 0:
            parts.append(text[start:i].strip())
            start = i + 1
    parts.append(text[start:].strip())
    return parts
