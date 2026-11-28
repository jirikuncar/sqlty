"""The ``check_source`` result type, ``Diagnostic``."""

from __future__ import annotations

from typing import TypedDict


class Diagnostic(TypedDict):
    """One ``check_source`` result: a plain dict, so ``diag["rule"]`` keeps working.

    ``severity`` is ``"error"`` or ``"warn"``. ``line`` is 1-based: where the call or the
    string it is about starts, the line a ``# sqlty: ignore`` comment silences it on.
    ``start`` and ``end`` are the byte offsets (UTF-8) of what it is about: a row type, a
    query, a parameter type or a comment. ``fix`` holds the ``(start, end, replacement)``
    byte edits of ``sqlty check --fix``; it is empty when no fix applies.
    """

    rule: str
    message: str
    severity: str
    line: int
    start: int
    end: int
    fix: list[tuple[int, int, str]]
