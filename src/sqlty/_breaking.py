"""``sqlty breaking``: the queries and schema of two git revisions, checked across.

During a rolling deploy the code of one revision runs against the schema of another.
``--against`` names the baseline ``A``; ``--head`` (the working tree by default) names
``B``. The backward check reads the queries of ``A`` against the schema of ``B`` (migrate
first), the forward check the queries of ``B`` against the schema of ``A`` (deploy
first). Each reports only what the other schema breaks: diagnostics a query does not
already get against its own schema.

Every revision but the working tree is checked out with ``git worktree add --detach``
into a temporary directory, removed afterwards, so the working tree is never touched.
"""

from __future__ import annotations

import contextlib
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Generator
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    import argparse

    from sqlty._cli import Settings, SettingsResolver
    from sqlty._lint import Diagnostic

Direction = Literal["backward", "forward", "both"]


class GitError(Exception):
    """A git command failed: not a repository, or an unknown revision."""


@dataclass
class Revision:
    """A checkout of the project at one revision, read as ``sqlty check`` reads it."""

    label: str
    root: Path
    resolver: SettingsResolver
    files: list[Path]
    paths: list[Path]

    def settings(self, relative: Path) -> Settings:
        return self.resolver.for_file(self.root / relative)


@dataclass(frozen=True)
class Finding:
    direction: Literal["backward", "forward"]
    path: Path
    diagnostic: Diagnostic


def run(args: argparse.Namespace) -> list[Finding]:
    """The findings of ``sqlty breaking``, for the paths and revisions ``args`` name."""
    root = Path(_git(Path.cwd(), "rev-parse", "--show-toplevel")).resolve()
    paths = [Path(p).resolve().relative_to(root) for p in (args.paths or [Path.cwd()])]
    with contextlib.ExitStack() as stack:
        against = _revision(args.against, stack.enter_context(_worktree(root, args.against)), paths)
        head_root = stack.enter_context(_worktree(root, args.head)) if args.head else root
        head = _revision(args.head or "the working tree", head_root, paths)
        findings: list[Finding] = []
        if args.direction in ("backward", "both"):
            findings += _compare("backward", queries=against, schema=head)
        if args.direction in ("forward", "both"):
            findings += _compare("forward", queries=head, schema=against)
        return findings


def report(
    findings: list[Finding],
    against: str,
    head: str | None,
    quiet: bool,
    output_format: Literal["text", "json", "github"] = "text",
) -> None:
    """Print each finding: ``path:line: [rule] message (direction)``, a JSON array with a
    ``direction`` per finding, or GitHub Actions annotations, all errors."""
    import json

    from sqlty._cli import github_annotation

    if quiet:
        return
    head = head or "the working tree"
    titles = {
        "backward": f"the queries of {against} against the schema of {head}",
        "forward": f"the queries of {head} against the schema of {against}",
    }
    if output_format == "json":
        rows = [{"file": str(f.path), **f.diagnostic, "direction": f.direction} for f in findings]
        print(json.dumps(rows, indent=2))
        return
    for finding in findings:
        d = finding.diagnostic
        message = f"{d['message']} ({finding.direction}: {titles[finding.direction]})"
        if output_format == "github":
            print(github_annotation(finding.path, {**d, "message": message, "severity": "error"}))
        else:
            print(f"{finding.path}:{d.get('line', '')}: [{d['rule']}] {message}")
    if findings and output_format == "text":
        print(f"Found {len(findings)} breaking change(s)")


def _compare(
    direction: Literal["backward", "forward"], *, queries: Revision, schema: Revision
) -> list[Finding]:
    from sqlty._cli import _module_names, read_source
    from sqlty._source import ParsedSource
    from sqlty.core import HelperIndex, breaking_source

    roots = [queries.root / p for p in queries.paths]
    sources: dict[Path, ParsedSource] = {}
    index = HelperIndex()
    for path in queries.files:
        if (text := read_source(path)) is None:
            continue
        names = _module_names(queries.resolver.resolve(path), roots)
        sources[path] = ParsedSource(
            text, names[0] + (".__init__" if path.name == "__init__.py" else "")
        )
        for name in names:
            index.add(name, sources[path])
    configured: dict[tuple[tuple[str, str, str | None], ...], HelperIndex] = {}
    findings: list[Finding] = []
    for path, parsed in sources.items():
        relative = path.relative_to(queries.root)
        own = queries.settings(relative)
        other = schema.settings(relative)
        if own.functions not in configured:
            configured[own.functions] = helpers = index.fork()
            for import_path, query_param, return_param in own.functions:
                helpers.register(import_path, query_param, return_param)
        for diagnostic in breaking_source(
            parsed,
            schema_sql=own.schema,
            against_sql=other.schema,
            dialect=own.dialect,
            rules=own.rules,
            import_style=own.import_style,
            untyped=own.untyped,
            type_modules=own.type_modules,
            helpers=configured[own.functions],
        ):
            findings.append(Finding(direction, relative, diagnostic))
    return findings


def _revision(label: str, root: Path, paths: list[Path]) -> Revision:
    from sqlty._cli import SettingsResolver, _collect_python_files, build_parser

    args = build_parser().parse_args(["check", *(str(root / p) for p in paths)])
    with contextlib.chdir(root):
        resolver = SettingsResolver(args)
    files = _collect_python_files([root / p for p in paths], resolver.source)
    return Revision(label, root, resolver, [f.resolve() for f in files], paths)


@contextlib.contextmanager
def _worktree(root: Path, ref: str) -> Generator[Path]:
    """``ref`` checked out into a temporary worktree, removed on exit."""
    _git(root, "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}")
    directory = Path(tempfile.mkdtemp(prefix="sqlty-breaking-"))
    try:
        _git(root, "worktree", "add", "--detach", "--quiet", str(directory), ref)
        yield directory.resolve()
    finally:
        with contextlib.suppress(GitError):
            _git(root, "worktree", "remove", "--force", str(directory))
        shutil.rmtree(directory, ignore_errors=True)
        with contextlib.suppress(GitError):
            _git(root, "worktree", "prune")


def _git(cwd: Path, *args: str) -> str:
    try:
        done = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=False)
    except FileNotFoundError as exc:
        raise GitError("git is not installed") from exc
    if done.returncode != 0:
        message = done.stderr.strip() or f"git {' '.join(args)} failed"
        if args[:2] == ("rev-parse", "--verify"):
            message = f"unknown revision {args[-1].removesuffix('^{commit}')!r}"
        raise GitError(message)
    return done.stdout.strip()


def main(args: argparse.Namespace) -> None:
    from sqlty._cli import EXIT_FOUND
    from sqlty._config import ConfigError

    try:
        findings = run(args)
    except GitError as exc:
        raise ConfigError(str(exc)) from exc
    report(findings, args.against, args.head, args.quiet, args.output_format)
    if findings:
        sys.exit(EXIT_FOUND)
