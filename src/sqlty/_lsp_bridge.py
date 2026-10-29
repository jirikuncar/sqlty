"""The Python side of the language server: checks and fixes of an editor's buffers.

A buffer's text may differ from its file on disk. The rest of the project is read from
disk once, as ``sqlty check ROOT`` reads it, and kept up to date with each buffer passed.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

from sqlty._cli import Project, build_parser, read_source
from sqlty._lsp_breaking import Baseline

if TYPE_CHECKING:
    from sqlty._breaking import Direction
    from sqlty._lint import Diagnostic


class Workspace:
    """The project under ``root``, with its config, schema and helpers; with ``against``,
    also what ``sqlty breaking --against`` reports.

    Example:
        >>> import tempfile
        >>> with tempfile.TemporaryDirectory() as root:
        ...     workspace = Workspace(root)
        ...     text = 'q = "--sql\\\\nSELECT 1.5::REAL"'
        ...     rules = [d["rule"] for d in workspace.check(f"{root}/q.py", text)]
        ...     fixed = workspace.fix(f"{root}/q.py", text)
        >>> rules
        ['stale-rewrite']
        >>> fixed.splitlines()[-1]
        'q = SQL[tuple[float]]("--sql\\\\nSELECT 1.5::REAL")'
    """

    def __init__(
        self, root: str, against: str | None = None, direction: Direction = "both"
    ) -> None:
        self.project = Project(build_parser().parse_args(["check", root]))
        self.baseline = Baseline(Path(root), against, direction) if against else None

    def check(self, path: str, text: str) -> list[Diagnostic]:
        """Diagnostics of the buffer ``text`` of the file ``path``."""
        file = Path(path)
        self.project.update(file, text)
        found = self.project.check(file)
        if self.baseline is not None:
            found += self.baseline.forward(self.project, file)
            found += self.baseline.backward_of(file, text)
        return found

    def breaking(self) -> dict[str, list[Diagnostic]]:
        """The backward findings of each file, at their spans in its text: the baseline's
        queries checked against this schema. ``check`` includes those of its buffer."""
        if self.baseline is None:
            return {}
        return {
            str(path): [*self.baseline.backward_of(path, self._text(path))]
            for path in self.baseline.check_backward(self.project)
        }

    def fix(self, path: str, text: str) -> str:
        """``text`` as ``sqlty check --fix`` would write it."""
        file = Path(path)
        self.project.update(file, text)
        return self.project.fix(file)

    def _text(self, path: Path) -> str:
        if (parsed := self.project.sources.get(path)) is not None:
            return parsed.text
        return read_source(path) or ""
