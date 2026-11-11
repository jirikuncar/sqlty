"""The text of one file, with what the checks ask of it besides the Rust engine.

    parsed = ParsedSource(text, module="pkg.queries")
    parsed.text             # the source text
    parsed.module           # the dotted module name, for relative imports
    parsed.ignores          # {line: Ignore} for "# sqlty: ignore" comments
    parsed.position(offset) # the line and column of a byte offset

Pass the object wherever a function takes ``source: str | ParsedSource`` (``check_source``,
``HelperIndex.add``); a plain ``str`` is wrapped with ``ParsedSource.of``. The syntax tree
is read in Rust (``sqlty_python``), never here.

Example:
    >>> ParsedSource("x = 1  # sqlty: ignore[a, b]").ignores[1].rules
    ('a', 'b')
    >>> ParsedSource("x = 1").may_define_helpers
    False
"""

from __future__ import annotations

import bisect
import functools
import io
import itertools
import re
import sys
import tokenize
from dataclasses import dataclass

from sqlty._core import module_facts, module_ignores

_IGNORE_RE = re.compile(r"#\s*sqlty:\s*ignore\b(?:\[([^\]]*)\])?")


@dataclass(frozen=True)
class Ignore:
    """A ``# sqlty: ignore`` comment; ``rules`` is ``None`` for a bare one.

    ``start`` and ``end`` are the byte offsets of the comment.
    """

    line: int
    rules: tuple[str, ...] | None
    start: int
    end: int


def _wrapper_name(import_style: str | None) -> str | None:
    """How the engine spells the wrapper for ``import_style``; ``None`` when it rejects it."""
    style = (import_style or "").strip()
    if style in ("", "from sqlty import SQL"):
        return "SQL"
    if style == "import sqlty":
        return "sqlty.SQL"
    for prefix, suffix in (("from sqlty import SQL as ", ""), ("import sqlty as ", ".SQL")):
        if style.startswith(prefix):
            return style[len(prefix) :].strip() + suffix
    return None


class ParsedSource:
    """Source text with the facts the checks cache per file.

    ``module`` is the dotted name of the file, used to resolve relative imports.
    """

    def __init__(self, text: str, module: str | None = None) -> None:
        self.text = text
        self.module = module

    @classmethod
    def of(cls, source: str | ParsedSource, module: str | None = None) -> ParsedSource:
        """``source`` itself when it is already a ``ParsedSource``, else a new one."""
        return source if isinstance(source, ParsedSource) else cls(source, module)

    def position(self, offset: int) -> tuple[int, int]:
        """The 1-based line and column of the byte ``offset``; the column counts characters.

        Example:
            >>> ParsedSource("x = 1\\nf('é', g)").position(14)
            (2, 8)
        """
        starts = self._byte_lines
        line = max(bisect.bisect_right(starts, offset, hi=len(starts) - 1), 1)
        column = len(self._bytes[starts[line - 1] : offset].decode(errors="replace"))
        return line, column + 1

    def offset(self, line: int, column: int) -> int:
        """The byte offset of the 1-based ``line`` and ``column`` (in characters), as ``position`` gives.

        A column past the end of its line stops at the line break.

        Example:
            >>> ParsedSource("x = 1\\nf('é', g)").offset(2, 8)
            14
        """
        starts = self._byte_lines
        line = min(max(line, 1), len(starts) - 1)
        text = self._bytes[starts[line - 1] : starts[line]].decode(errors="replace")
        text = text[: max(column - 1, 0)].split("\n")[0].split("\r")[0]
        return starts[line - 1] + len(text.encode())

    @functools.cached_property
    def _bytes(self) -> bytes:
        return self.text.encode()

    @functools.cached_property
    def _byte_lines(self) -> list[int]:
        """Byte offset of each line start, and the length after the last."""
        return list(
            itertools.accumulate(map(len, self._bytes.splitlines(keepends=True)), initial=0)
        )

    @functools.cached_property
    def may_define_helpers(self) -> bool:
        """Whether the text can define a marked helper (needs ``Annotated`` and ``sqlty``)."""
        return "Annotated" in self.text and "sqlty" in self.text and "SQL" in self.text

    def may_wrap(self, import_style: str | None = None) -> bool:
        """Whether the text can hold a ``SQL[T](...)`` call, spelled as ``import_style`` says."""
        prefix = "from sqlty import SQL as "
        style = (import_style or "").strip()
        return (style[len(prefix) :].strip() if style.startswith(prefix) else "SQL") in self.text

    def any_calls(self, import_style: str | None = None) -> list[int]:
        """Byte offset of each ``SQL[T](...)`` call whose ``T`` is ``typing.Any``.

        Example:
            >>> ParsedSource("import typing as t\\nq = SQL[t.Any](x)\\nr = SQL[int](y)").any_calls()
            [23]
        """
        wrapper = _wrapper_name(import_style)
        if wrapper is None or "Any" not in self.text or not self.may_wrap(import_style):
            return []
        facts = module_facts(self.text)
        return [
            offset
            for offset, name in (() if facts is None else facts[3])
            if name == wrapper or name.endswith(f".{wrapper}")
        ]

    @property
    def has_ignore_comments(self) -> bool:
        """Whether the text can hold a ``# sqlty: ignore`` comment."""
        return "sqlty:" in self.text

    @functools.cached_property
    def ignores(self) -> dict[int, Ignore]:
        """``# sqlty: ignore`` comments by line; read only when the text contains ``sqlty:``."""
        if not self.has_ignore_comments:
            return {}
        found = module_ignores(self.text, sys.version_info[:2])
        if found is None:
            return self._tokenized_ignores()
        return {
            line: Ignore(line, None if rules is None else tuple(rules), start, end)
            for line, rules, start, end in found
        }

    def _tokenized_ignores(self) -> dict[int, Ignore]:
        """``ignores`` of a source that does not parse: the comments up to the error."""
        found: dict[int, Ignore] = {}
        tokens = tokenize.generate_tokens(io.StringIO(self.text).readline)
        try:
            for tok in tokens:
                if tok.type != tokenize.COMMENT or (match := _IGNORE_RE.match(tok.string)) is None:
                    continue
                names = match.group(1)
                rules = (
                    None
                    if names is None
                    else tuple(n.strip() for n in names.split(",") if n.strip())
                )
                line, column = tok.start
                start = self._byte_lines[line - 1] + len(tok.line[:column].encode())
                found[line] = Ignore(line, rules, start, start + len(tok.string.encode()))
        except tokenize.TokenError, SyntaxError:
            pass
        return found
