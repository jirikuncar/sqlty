"""The Python side of the language server: checks and fixes of an editor's buffers.

A buffer's text may differ from its file on disk. The rest of the project is read from
disk once, as ``sqlty check ROOT`` reads it, and kept up to date with each buffer passed.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

from sqlty._cli import Project, build_parser

if TYPE_CHECKING:
    from sqlty._lint import Diagnostic


class Workspace:
    """The project under ``root``, with its config, schema and helpers.

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

    def __init__(self, root: str) -> None:
        self.project = Project(build_parser().parse_args(["check", root]))

    def check(self, path: str, text: str) -> list[Diagnostic]:
        """Diagnostics of the buffer ``text`` of the file ``path``."""
        file = Path(path)
        self.project.update(file, text)
        return self.project.check(file)

    def fix(self, path: str, text: str) -> str:
        """``text`` as ``sqlty check --fix`` would write it."""
        file = Path(path)
        self.project.update(file, text)
        return self.project.fix(file)
