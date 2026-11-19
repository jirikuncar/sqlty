"""One Python source file and what Rust reads of it, each read once and cached.

Usage:
    parsed = ParsedSource(text, module="pkg.mod")
    parsed.facts            # imports, typing aliases and calls; None when Rust declines the text
    parsed.typing_aliases   # local names of typing members: {'t': 'typing', 'A': 'Any'}
    parsed.imported_names   # absolute imports, local name to dotted name: {'D': 'decimal.Decimal'}

Pass the object wherever a function takes ``source: str | ParsedSource``
(``check_source``, ``HelperIndex.add``, ``extract_type_definitions``); a plain ``str`` is
wrapped with ``ParsedSource.of``. Build one per file and reuse it.

Example:
    >>> parsed = ParsedSource("import typing as t\\nclass A: ...\\nf(A)")
    >>> parsed.typing_aliases
    {'t': 'typing'}
    >>> ParsedSource("x = 1").may_define_helpers
    False
"""

from __future__ import annotations

import bisect
import functools
import itertools
import typing

from sqlty._core import module_facts


class Facts(typing.NamedTuple):
    """What ``sqlty_python::module::facts`` reads of a module, see ``ParsedSource.facts``."""

    names: list[str]
    attributes: list[str]
    imports: list[tuple[str, str, str | None, int]]
    any_calls: list[tuple[int, str]]
    typing_aliases: list[tuple[str, str]]
    driver_calls: list[
        tuple[
            int,
            str,
            list[tuple[int | str, str | None]] | None,
            str,
            str | None,
            tuple[int, int],
        ]
    ]


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
    """Source text with what Rust reads of it, read on first use.

    ``module`` is the dotted name of the file, used to resolve relative imports.
    """

    def __init__(self, text: str, module: str | None = None) -> None:
        self.text = text
        self.module = module

    @classmethod
    def of(cls, source: str | ParsedSource, module: str | None = None) -> ParsedSource:
        """``source`` itself when it is already parsed, else a new ``ParsedSource``."""
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
    def typing_aliases(self) -> dict[str, str]:
        """Local names for ``typing`` / ``typing_extensions`` and their members, to originals."""
        return {} if self.facts is None else dict(self.facts.typing_aliases)

    @functools.cached_property
    def imported_names(self) -> dict[str, str]:
        """Local names bound by absolute imports, to the dotted names they stand for; the outermost binding wins.

        Example:
            >>> ParsedSource("import a.b, c as d\\nfrom e.f import g as h").imported_names
            {'a': 'a', 'd': 'c', 'h': 'e.f.g'}
        """
        names: dict[str, str] = {}
        for local, original, module, level in [] if self.facts is None else self.facts.imports:
            if module is None:
                root = original.partition(".")[0]
                names.setdefault(*((root, root) if local == original else (local, original)))
            elif module and not level:
                names.setdefault(local, f"{module}.{original}")
        return names

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
        """Byte offset of each ``SQL[T](...)`` call whose ``T`` is ``typing.Any``, quoted or in ``Annotated`` too.

        Example:
            >>> ParsedSource("import typing as t\\nq = SQL[t.Any](x)\\nr = SQL[int](y)").any_calls()
            [23]
        """
        wrapper = _wrapper_name(import_style)
        if (
            wrapper is None
            or "Any" not in self.text
            or not self.may_wrap(import_style)
            or self.facts is None
        ):
            return []
        return [
            offset
            for offset, name in self.facts.any_calls
            if name == wrapper or name.endswith(f".{wrapper}")
        ]

    @functools.cached_property
    def facts(self) -> Facts | None:
        """What Rust reads of the module, ``None`` when it cannot parse the text."""
        found = module_facts(self.text)
        return None if found is None else Facts(*found)
