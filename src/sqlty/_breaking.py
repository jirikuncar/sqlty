"""``sqlty breaking``: the queries and schema of two revisions, checked across.

During a rolling deploy the code of one revision runs against the schema of another.
``--against`` names the baseline ``A``; ``--head`` (the working tree by default) names
``B``. The backward check reads the queries of ``A`` against the schema of ``B`` (migrate
first), the forward check the queries of ``B`` against the schema of ``A`` (deploy
first). Each reports only what the other schema breaks: diagnostics a query does not
already get against its own schema.

Every revision but the working tree is checked out with ``git worktree add --detach``
into a temporary directory, removed afterwards, so the working tree is never touched.
A baseline can also be a snapshot ``--save`` wrote, which needs no git history, or a
schema file, which has no queries and so is only checked forward.
"""

from __future__ import annotations

import contextlib
import os
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Generator
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import TYPE_CHECKING, Literal, NamedTuple, Protocol

if TYPE_CHECKING:
    import argparse

    from sqlty._cli import Settings, SettingsResolver
    from sqlty._helpers import HelperIndex
    from sqlty._lint import Diagnostic
    from sqlty._snapshot import Snapshot
    from sqlty._source import ParsedSource
    from sqlty.core import Schema

    _Work = tuple[Path, ParsedSource, Settings, Schema | None, HelperIndex]

Direction = Literal["backward", "forward", "both"]
SchemaKey = tuple[str | None, str, tuple[tuple[str, str], ...]]


class GitError(Exception):
    """A git command failed: not a repository, or an unknown revision."""


class Source(NamedTuple):
    """A Python file of a revision, with the module names its helpers are imported by, or a
    ``.sql`` file of named queries, which has none."""

    text: str
    module: str
    names: tuple[str, ...]


class Queries(Protocol):
    """A revision whose Python files are checked: a git revision or a snapshot."""

    label: str

    def sources(self) -> dict[Path, Source]: ...

    def settings(self, relative: Path) -> Settings: ...

    def roots(self) -> list[Path]: ...


class Schemas(Protocol):
    """A revision whose schema the other revision's queries are checked against."""

    label: str

    def schema(self, relative: Path, own: Settings) -> Schema | None: ...

    def schema_key(self, relative: Path, own: Settings) -> SchemaKey: ...


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

    def schema(self, relative: Path, own: Settings) -> Schema | None:
        return self.settings(relative).schema

    def schema_key(self, relative: Path, own: Settings) -> SchemaKey:
        return self.settings(relative).schema_key

    def roots(self) -> list[Path]:
        from sqlty._cli import _checked_roots

        return _checked_roots([self.root / p for p in self.paths], self.files)

    def sources(self) -> dict[Path, Source]:
        from sqlty._cli import _module_names, read_source

        roots = [self.root / p for p in self.paths]
        sources: dict[Path, Source] = {}
        for path in self.files:
            if (text := read_source(path)) is None:
                continue
            if path.suffix == ".sql":
                sources[path.relative_to(self.root)] = Source(text, "", ())
                continue
            names = _module_names(self.resolver.resolve(path), roots)
            module = names[0] + (".__init__" if path.name == "__init__.py" else "")
            sources[path.relative_to(self.root)] = Source(text, module, tuple(names))
        return sources


@dataclass
class SchemaFiles:
    """A schema without queries, such as a dump or migrations: a baseline only forward."""

    label: str
    sql: str
    loaded: dict[tuple[str, tuple[tuple[str, str], ...]], Schema] = field(default_factory=dict)

    def schema(self, relative: Path, own: Settings) -> Schema:
        from sqlty.core import load_schema

        key = (own.dialect, tuple(sorted((own.types or {}).items())))
        if key not in self.loaded:
            self.loaded[key] = load_schema(self.sql, dialect=own.dialect, types=own.types)
        return self.loaded[key]

    def schema_key(self, relative: Path, own: Settings) -> SchemaKey:
        return replace(own, schema_sql=self.sql).schema_key


@dataclass(frozen=True)
class Finding:
    direction: Literal["backward", "forward"]
    against: str
    path: Path
    diagnostic: Diagnostic
    source: ParsedSource


def run(args: argparse.Namespace) -> list[Finding]:
    """The findings of ``sqlty breaking``, for the paths and revisions ``args`` name; with
    ``--save``, the snapshot of ``--head`` is written first."""
    from sqlty._snapshot import save

    root = _root(args)
    jobs = args.jobs or os.process_cpu_count() or 1
    paths = [Path(p).resolve().relative_to(root) for p in (args.paths or [Path.cwd()])]
    with contextlib.ExitStack() as stack:
        baselines = _baselines(args.against, root, paths, stack) if args.against else []
        head_root = stack.enter_context(_worktree(root, args.head)) if args.head else root
        head = _revision(args.head or "the working tree", head_root, paths)
        if args.save:
            save(head, args.save)
        findings: list[Finding] = []
        for baseline in baselines:
            if args.direction in ("backward", "both"):
                if isinstance(baseline, SchemaFiles):
                    print(
                        f"sqlty: {baseline.label} has no queries; skipping the backward check",
                        file=sys.stderr,
                    )
                else:
                    findings += _compare(
                        "backward", baseline.label, queries=baseline, schema=head, jobs=jobs
                    )
            if args.direction in ("forward", "both"):
                findings += _compare(
                    "forward", baseline.label, queries=head, schema=baseline, jobs=jobs
                )
        return findings


def report(
    findings: list[Finding],
    head: str | None,
    quiet: bool,
    output_format: Literal["text", "json", "github", "sarif"] = "text",
) -> None:
    """Print each finding: ``path:line: [rule] message (direction)``, a JSON array with a
    ``direction`` and ``against`` per finding, GitHub Actions annotations at each
    finding's severity, or a SARIF log."""
    import json

    from sqlty._cli import github_annotation, json_diagnostic
    from sqlty._sarif import sarif_log

    if quiet:
        return
    head = head or "the working tree"

    def message(finding: Finding) -> str:
        if finding.direction == "backward":
            title = f"the queries of {finding.against} against the schema of {head}"
        else:
            title = f"the queries of {head} against the schema of {finding.against}"
        return f"{finding.diagnostic['message']} ({finding.direction}: {title})"

    if output_format in ("json", "sarif"):
        rows = [
            {
                "file": str(f.path),
                **json_diagnostic(f.diagnostic, f.source),
                **({"message": message(f)} if output_format == "sarif" else {}),
                "direction": f.direction,
                "against": f.against,
            }
            for f in findings
        ]
        print(json.dumps(rows if output_format == "json" else sarif_log(rows), indent=2))
        return
    for finding in findings:
        d = finding.diagnostic
        if output_format == "github":
            print(github_annotation(finding.path, {**d, "message": message(finding)}))
        else:
            print(f"{finding.path}:{d.get('line', '')}: [{d['rule']}] {message(finding)}")
    if findings and output_format == "text":
        print(f"Found {len(findings)} breaking change(s)")


def _compare(
    direction: Literal["backward", "forward"],
    against: str,
    *,
    queries: Queries,
    schema: Schemas,
    jobs: int = 1,
) -> list[Finding]:
    """What ``schema`` breaks in ``queries``; up to ``jobs`` processes check the files."""
    from sqlty._helpers import configured_helpers
    from sqlty._source import ParsedSource
    from sqlty.core import HelperIndex

    sources: dict[Path, ParsedSource] = {}
    index = HelperIndex()
    for relative, source in queries.sources().items():
        sources[relative] = ParsedSource(source.text, source.module or None)
        for name in source.names:
            index.add(name, sources[relative])
    helpers_for = configured_helpers(index, queries.roots())
    work: list[_Work] = []
    for relative, parsed in sources.items():
        own = queries.settings(relative)
        if schema.schema_key(relative, own) != own.schema_key:
            work.append((relative, parsed, own, schema.schema(relative, own), helpers_for(own)))
    return [
        Finding(direction, against, relative, diagnostic, parsed)
        for (relative, parsed, *_), found in zip(work, _map(work, jobs), strict=True)
        for diagnostic in found
    ]


_shared: list[_Work] = []


def _map(work: list[_Work], jobs: int) -> list[list[Diagnostic]]:
    """The diagnostics of each item of ``work``, checked by forked processes that inherit it.

    The checks are mostly Python, so threads would wait on the GIL. Without ``fork`` (Windows),
    or for one job, they run here.
    """
    import multiprocessing
    from concurrent.futures import ProcessPoolExecutor

    jobs = min(jobs, len(work))
    if jobs <= 1 or "fork" not in multiprocessing.get_all_start_methods():
        return [_check(item) for item in work]
    _shared[:] = work
    try:
        context = multiprocessing.get_context("fork")
        size = max(1, min(64, len(work) // (jobs * 4)))
        chunks = [range(i, min(i + size, len(work))) for i in range(0, len(work), size)]
        with ProcessPoolExecutor(jobs, mp_context=context) as pool:
            return [found for chunk in pool.map(_check_shared, chunks) for found in chunk]
    finally:
        _shared.clear()


def _check_shared(chunk: range) -> list[list[Diagnostic]]:
    return [_check(_shared[i]) for i in chunk]


def _check(item: _Work) -> list[Diagnostic]:
    from sqlty._queries import breaking_query_file
    from sqlty.core import breaking_source

    relative, parsed, own, against, helpers = item
    if relative.suffix == ".sql":
        return breaking_query_file(
            parsed.text,
            name=relative.stem,
            query_format=own.query_format,
            dialect=own.dialect,
            schema_sql=own.schema,
            against_sql=against,
            rules=own.rules,
        )
    return breaking_source(
        parsed,
        schema_sql=own.schema,
        against_sql=against,
        dialect=own.dialect,
        rules=own.rules,
        import_style=own.import_style,
        untyped=own.untyped,
        query_variants=own.query_variants,
        driver_calls=own.driver_calls,
        helpers=helpers,
    )


def _baselines(
    against: str, root: Path, paths: list[Path], stack: contextlib.ExitStack
) -> list[Revision | Snapshot | SchemaFiles]:
    """What ``--against`` names: a git revision when no such path exists, else a snapshot,
    each snapshot of a directory, or schema files."""
    from sqlty._cli import _load_schema
    from sqlty._config import ConfigError
    from sqlty._snapshot import Snapshot

    path = Path(against)
    if not path.exists():
        return [_revision(against, stack.enter_context(_worktree(root, against)), paths)]
    if path.suffix == ".json" and path.is_file():
        return [Snapshot.load(path, against, paths)]
    if path.is_dir() and (snapshots := sorted(path.glob("*.json"))):
        return [Snapshot.load(p, p.stem, paths) for p in snapshots]
    if (sql := _load_schema(str(path), Path.cwd())) is None:
        raise ConfigError(f"{against} has no schema .sql files")
    return [SchemaFiles(against, sql)]


def _root(args: argparse.Namespace) -> Path:
    """The repository's top level; outside one, the current directory, unless a git
    revision is needed."""
    try:
        return Path(_git(Path.cwd(), "rev-parse", "--show-toplevel")).resolve()
    except GitError:
        if args.head or (args.against and not Path(args.against).exists()):
            raise
        return Path.cwd().resolve()


def _revision(label: str, root: Path, paths: list[Path]) -> Revision:
    from sqlty._cli import (
        SettingsResolver,
        _collect_python_files,
        _collect_query_files,
        build_parser,
    )

    args = build_parser().parse_args(["check", *(str(root / p) for p in paths)])
    with contextlib.chdir(root):
        resolver = SettingsResolver(args)
        files = _collect_python_files([root / p for p in paths], resolver.source)
        files += _collect_query_files([root / p for p in paths], files, resolver)
        files = [f.resolve() for f in files]
    return Revision(label, root, resolver, files, paths)


@contextlib.contextmanager
def _worktree(root: Path, ref: str) -> Generator[Path]:
    """``ref`` checked out into a temporary worktree, removed on exit."""
    _git(root, "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}")
    directory = Path(tempfile.mkdtemp(prefix="sqlty-breaking-"))
    # Named like the repository, so its files have the module names they have there.
    checkout = directory / root.name
    try:
        _git(root, "worktree", "add", "--detach", "--quiet", str(checkout), ref)
        yield checkout.resolve()
    finally:
        with contextlib.suppress(GitError):
            _git(root, "worktree", "remove", "--force", str(checkout))
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

    if args.against is None and args.save is None:
        raise ConfigError("sqlty breaking needs --against, --save or both")
    try:
        findings = run(args)
    except GitError as exc:
        raise ConfigError(str(exc)) from exc
    report(findings, args.head, args.quiet, args.output_format)
    if any(f.diagnostic["severity"] == "error" for f in findings):
        sys.exit(EXIT_FOUND)
