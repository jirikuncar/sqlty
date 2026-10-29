"""``sqlty.breaking.against`` in the editor: ``sqlty breaking`` for the buffers of a workspace.

The forward check (the buffer's queries against the baseline's schema) runs with every
check. The backward check (the baseline's queries against the workspace's schema) does
not depend on the buffers: it runs once, when the workspace is loaded, which a saved
schema or config does again.
"""

from __future__ import annotations

import contextlib
import weakref
from pathlib import Path
from typing import TYPE_CHECKING

from sqlty._breaking import (
    Direction,
    Finding,
    GitError,
    Revision,
    SchemaFiles,
    _baselines,
    _compare,
    _git,
)
from sqlty._config import ConfigError
from sqlty._lint import Diagnostic

if TYPE_CHECKING:
    from sqlty._cli import Project


class SourcedDiagnostic(Diagnostic, total=False):
    """A diagnostic and the check it comes from, when that is not ``sqlty check``."""

    source: str


class Baseline:
    """The revision, snapshot or schema ``against`` names, for the workspace at ``root``."""

    def __init__(self, root: Path, against: str, direction: Direction) -> None:
        self.root = root
        self.direction = direction
        try:
            self.repository = Path(_git(root, "rev-parse", "--show-toplevel")).resolve()
        except GitError as exc:
            if not (root / against).exists():
                message = f"sqlty.breaking.against: {against} does not exist, and {root} is in no git repository"
                raise ConfigError(message) from exc
            self.repository = root.resolve()
        self.prefix = root.resolve().relative_to(self.repository)
        stack = contextlib.ExitStack()
        weakref.finalize(self, stack.close)
        try:
            with contextlib.chdir(root):
                self.baselines = _baselines(against, self.repository, [self.prefix], stack)
        except GitError as exc:
            raise ConfigError(f"sqlty.breaking.against: {exc}") from exc
        self.backward: dict[Path, list[Finding]] = {}

    def forward(self, project: Project, path: Path) -> list[SourcedDiagnostic]:
        """What the baseline's schema breaks in the queries of ``path``."""
        from sqlty.core import breaking_source

        relative = self._relative(path)
        if relative is None or self.direction == "backward":
            return []
        settings = project.settings[path]
        found: list[SourcedDiagnostic] = []
        for baseline in self.baselines:
            for diagnostic in breaking_source(
                project.sources[path],
                schema_sql=settings.schema,
                against_sql=baseline.schema(relative, settings),
                dialect=settings.dialect,
                rules=settings.rules,
                import_style=settings.import_style,
                untyped=settings.untyped,
                query_variants=settings.query_variants,
                helpers=project.helpers[path],
            ):
                message = f"{diagnostic['message']} (with the schema of {baseline.label})"
                found.append(
                    {
                        **diagnostic,
                        "message": message,
                        "fix": [],
                        "source": "sqlty breaking (forward)",
                    }
                )
        return found

    def check_backward(self, project: Project) -> list[Path]:
        """Check the baseline's queries against the workspace's schema; the files with findings."""
        if self.direction == "forward":
            return []
        head = Revision(
            "the workspace", self.repository, project.resolver, project.files, [self.prefix]
        )
        self.backward = {}
        for baseline in self.baselines:
            if isinstance(baseline, SchemaFiles):
                continue
            for finding in _compare("backward", baseline.label, queries=baseline, schema=head):
                if finding.path.is_relative_to(self.prefix):
                    path = self.root / finding.path.relative_to(self.prefix)
                    self.backward.setdefault(path, []).append(finding)
        return list(self.backward)

    def backward_of(self, path: Path, text: str) -> list[SourcedDiagnostic]:
        """The backward findings of ``path``: at their spans while ``text`` is the baseline's,
        at the top of the file otherwise."""
        found: list[SourcedDiagnostic] = []
        for finding in self.backward.get(path, []):
            diagnostic = finding.diagnostic
            if finding.source.text == text:
                message = f"{diagnostic['message']} (in the code of {finding.against})"
                found.append(
                    {
                        **diagnostic,
                        "message": message,
                        "fix": [],
                        "source": "sqlty breaking (backward)",
                    }
                )
            else:
                line = diagnostic.get("line")
                message = f"{diagnostic['message']} (in the code of {finding.against}, line {line})"
                found.append(
                    {
                        **diagnostic,
                        "message": message,
                        "start": 0,
                        "end": 0,
                        "fix": [],
                        "source": "sqlty breaking (backward)",
                    }
                )
        return found

    def _relative(self, path: Path) -> Path | None:
        resolved = path.resolve()
        return (
            resolved.relative_to(self.repository)
            if resolved.is_relative_to(self.repository)
            else None
        )
