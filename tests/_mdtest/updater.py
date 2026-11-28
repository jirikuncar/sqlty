"""Rewrite assertion comments in place for ``--update-mdtest``.

Edits are addressed by line numbers recorded at parse time; each applied
edit shifts the lines after it, so the updater maps original lines to
current ones. Sections may update in any order.
"""

from __future__ import annotations

from pathlib import Path

from .parser import Assertion


class Updater:
    def __init__(self, path: Path) -> None:
        self.path = path
        self._edits: list[tuple[int, int, int]] = []  # (start, end, delta), 0-based original
        self._done: set[object] = set()

    def replace(self, assertion: Assertion, values: list[str]) -> None:
        if not self._claim(("assertion", assertion.line)):
            return
        prefix = f"{assertion.indent}{assertion.comment} {assertion.kind}:"
        start = assertion.line - 1
        self._splice(start, start + assertion.span, [_join(prefix, v) for v in values])

    def _claim(self, key: object) -> bool:
        if key in self._done:
            return False
        self._done.add(key)
        return True

    def _at(self, line: int) -> int:
        return line + sum(d for _, e, d in self._edits if e <= line)

    def _splice(self, start: int, end: int, new: list[str]) -> None:
        # Untouched lines keep their bytes, and written ones the file's line ending.
        text = self.path.read_text(newline="")
        eol = "\r\n" if "\r\n" in text else "\n"
        lines = text.split("\n")
        at = self._at(start)
        lines[at : at + end - start] = [line + eol.removesuffix("\n") for line in new]
        self.path.write_text("\n".join(lines), newline="")
        self._edits.append((start, end, len(new) - (end - start)))


def _join(prefix: str, value: str) -> str:
    return f"{prefix} {value}" if value else prefix
