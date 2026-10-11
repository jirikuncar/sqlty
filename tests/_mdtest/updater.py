"""Rewrite assertion comments in place for ``--update-mdtest``.

Edits are addressed by line numbers recorded at parse time; each applied
edit shifts the lines after it, so the updater maps original lines to
current ones. Sections may update in any order.
"""

from __future__ import annotations

import re
from pathlib import Path

from .parser import Assertion

_TRAILING_ERROR = re.compile(r"^(.*?\S)\s+#\s+error:.*$")


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

    def replace_errors(
        self, assertions: list[Assertion], diagnostics: list[tuple[int, str, str]]
    ) -> None:
        if not self._claim(("errors", assertions[0].line)):
            return
        lines = self.path.read_text().splitlines()
        for a in assertions:
            start = a.line - 1
            if a.trailing:
                code = _TRAILING_ERROR.sub(r"\1", lines[self._at(start)])
                self._splice(start, start + 1, [code])
            else:
                self._splice(start, start + 1, [])
        if not diagnostics:
            first = min(assertions, key=lambda a: a.line)
            self._splice(first.line - 1, first.line - 1, [f"{first.indent}# error:"])
        for line, rule, message in sorted(diagnostics):
            target = self.path.read_text().splitlines()[self._at(line - 1)]
            indent = target[: len(target) - len(target.lstrip())]
            self._splice(line - 1, line - 1, [f'{indent}# error: [{rule}] "{message}"'])

    def _claim(self, key: object) -> bool:
        if key in self._done:
            return False
        self._done.add(key)
        return True

    def _at(self, line: int) -> int:
        return line + sum(d for _, e, d in self._edits if e <= line)

    def _splice(self, start: int, end: int, new: list[str]) -> None:
        lines = self.path.read_text().splitlines(keepends=True)
        at = self._at(start)
        lines[at : at + end - start] = [f"{line}\n" for line in new]
        self.path.write_text("".join(lines))
        self._edits.append((start, end, len(new) - (end - start)))


def _join(prefix: str, value: str) -> str:
    return f"{prefix} {value}" if value else prefix
