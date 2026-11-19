"""CLI entry point — ruff-style subcommand router.

Commands:
    sqlty rewrite [OPTIONS] [PATH...]   Rewrite source files
    sqlty strip [OPTIONS] [PATH...]     Strip SQL[T](...) wrappers
    sqlty format [OPTIONS] [PATH...]    Lay out query strings with sqruff
    sqlty check [OPTIONS] [PATH...]     Lint SQL type annotations
    sqlty rule [NAME]                   Explain a lint rule, or list them
    sqlty server                        Run the language server, for editors
    sqlty version                       Print version
"""

from __future__ import annotations

import functools
import json
import logging
import os
import sys
from dataclasses import dataclass, replace
from pathlib import Path
from typing import TYPE_CHECKING, NoReturn, cast

if TYPE_CHECKING:
    import argparse
    from collections.abc import Callable, Iterable

    from sqlty._breaking import SchemaKey
    from sqlty._check import RowUsage
    from sqlty._config import RuleOverride, SchemaSpec, SQLTyConfig
    from sqlty._core import Schema
    from sqlty._lint import Diagnostic
    from sqlty._source import ParsedSource

logger = logging.getLogger(__name__)

EXIT_FOUND = 1
"""``check`` reported an error, or ``--check``/``--diff`` found files to change."""
EXIT_ERROR = 2
"""Bad usage or configuration, a missing path, or an internal error."""


def main(argv: list[str] | None = None) -> None:
    """CLI entry point."""
    parser = build_parser()
    hard_exit = argv is None
    argv = sys.argv[1:] if argv is None else argv
    args = parser.parse_args(argv)
    args.hard_exit = hard_exit

    if args.version:
        _cmd_version()
        return

    if args.command is None:
        parser.print_help()
        sys.exit(0)

    from sqlty._config import ConfigError

    try:
        if (code := _rust(argv)) is not None:
            if hard_exit:
                _hard_exit(code)
            if code:
                sys.exit(code)
            return
        match args.command:
            case "complete":
                _cmd_complete(args)
            case "breaking":
                from sqlty._breaking import main as breaking

                breaking(args)
            case "server":
                from sqlty._core import serve

                # Messages own stdout: a stray print must not corrupt one.
                sys.stdout = sys.stderr
                serve()
    except ConfigError as exc:
        print(f"sqlty: error: {exc}", file=sys.stderr)
        sys.exit(EXIT_ERROR)
    except KeyboardInterrupt, SystemExit:
        raise
    except BaseException as exc:  # a Rust panic is a BaseException
        import traceback

        traceback.print_exc()
        print(
            "sqlty: internal error; please report it with the traceback above at "
            + issue_url(f"{type(exc).__name__}: {exc}"),
            file=sys.stderr,
        )
        sys.exit(EXIT_ERROR)


def issue_url(cause: str) -> str:
    """A new-issue link prefilling the bug report form, as ``sqlty_python::report`` does."""
    import platform
    from urllib.parse import quote, urlencode

    from sqlty import __version__

    title = cause.splitlines()[0] if cause else ""
    fields = {
        "title": f"Crash: {title[:80]}",
        "output": cause[:2000],
        "version": __version__,
        "environment": f"{platform.system()} {platform.machine()}, Python {platform.python_version()}",
    }
    return "https://github.com/jirikuncar/sqlty/issues/new?template=bug_report.yml&" + urlencode(
        fields, quote_via=quote, safe=""
    )


def _rust(argv: list[str]) -> int | None:
    """The exit code of ``argv`` run by the Rust CLI, or ``None`` for ``complete``,
    ``breaking`` and ``server``, which Python runs."""
    from sqlty._core import cli

    return cli(["sqlty", *argv])


def build_parser() -> argparse.ArgumentParser:
    """The ``sqlty`` argument parser; ``docs/reference/cli.md`` is rendered from it."""
    import argparse

    parser = argparse.ArgumentParser(
        prog="sqlty",
        description="Type-safe SQL queries for Python.",
    )
    parser.add_argument("--version", action="store_true", help="Print version and exit")

    subparsers = parser.add_subparsers(dest="command")

    # --- rewrite ---
    rewrite_parser = subparsers.add_parser(
        "rewrite",
        help='Rewrite "--sql\\n..." strings into SQL[T]("--sql\\n...") calls',
    )
    _add_common_args(rewrite_parser)
    rewrite_parser.add_argument(
        "--check",
        action="store_true",
        help="Do not modify files; print `Would rewrite <file>` and exit 1 if any file needs "
        "rewriting",
    )
    rewrite_parser.add_argument(
        "--diff",
        action="store_true",
        help="Print a unified diff instead of modifying files; exit 1 if any file would change",
    )
    rewrite_parser.add_argument(
        "--untyped",
        choices=["skip", "sentinel"],
        help="Row-returning queries of unknown row type: leave unwrapped (skip, default) "
        "or wrap as `SQL[UNTYPED](...)` (sentinel). Config: untyped in [tool.sqlty]",
    )

    # --- strip ---
    strip_parser = subparsers.add_parser(
        "strip",
        help='Strip SQL[T]("--sql\\n...") wrappers back to plain strings',
    )
    _add_common_args(strip_parser)
    strip_parser.add_argument(
        "--check",
        action="store_true",
        help="Do not modify files; print `Would strip <file>` and exit 1 if any file would change",
    )
    strip_parser.add_argument(
        "--diff",
        action="store_true",
        help="Print a unified diff instead of modifying files; exit 1 if any file would change",
    )

    # --- format ---
    format_parser = subparsers.add_parser(
        "format",
        help="Lay out query strings with sqruff: one plain triple-quoted literal per query",
    )
    _add_common_args(format_parser)
    format_parser.add_argument(
        "--check",
        action="store_true",
        help="Do not modify files; print `Would format <file>` and exit 1 if any file would change",
    )
    format_parser.add_argument(
        "--diff",
        action="store_true",
        help="Print a unified diff instead of modifying files; exit 1 if any file would change",
    )

    # --- check ---
    check_parser = subparsers.add_parser(
        "check",
        help="Lint: validate SQL type annotations and detect issues",
    )
    _add_common_args(check_parser)
    check_parser.add_argument(
        "--fix",
        action="store_true",
        help="Apply the fixable rules (see `sqlty rule`), write the files, then report what remains",
    )
    check_parser.add_argument(
        "--exit-zero",
        action="store_true",
        help="Exit 0 even when an error-severity diagnostic is reported",
    )
    check_parser.add_argument(
        "--exit-zero-on-warning",
        action="store_true",
        help="Exit 0 when no diagnostic has severity error (the default); cannot be combined "
        "with --error-on-warning",
    )
    check_parser.add_argument(
        "--error-on-warning",
        action="store_true",
        help="Exit 1 when any diagnostic has severity warn; cannot be combined with --exit-zero "
        "or --exit-zero-on-warning",
    )
    check_parser.add_argument(
        "--statistics",
        action="store_true",
        help="After the summary, print how many queries were typed, partially typed, unparsed "
        "and skipped",
    )
    check_parser.add_argument(
        "--explain",
        action="store_true",
        help="Under a type diagnostic, print the inference rule (id, reference, reasoning) "
        "behind each column's type and nullability",
    )
    check_parser.add_argument(
        "--output-format",
        choices=["text", "json", "github", "sarif"],
        default="text",
        help="How diagnostics are printed: text (file:line: [rule] message, the default), json "
        "(an array of objects with file, line, rule, severity, message, the start and end of what "
        "it underlines, and the fix edits), github (GitHub Actions annotations) or sarif (a "
        "SARIF 2.1.0 log, for GitHub code scanning). Summaries, --statistics and --explain are "
        "text only",
    )

    # --- complete ---
    complete_parser = subparsers.add_parser(
        "complete",
        help="List the table and column names that complete at FILE:LINE:COL in a query string "
        "(sqlty complete app.py:12:30), for scripts and agents",
    )
    complete_parser.add_argument(
        "location",
        type=_location,
        metavar="FILE:LINE:COL",
        help="The cursor: a Python file, a 1-based line and a 1-based column in characters",
    )
    _add_common_args(complete_parser)
    complete_parser.set_defaults(paths=None)
    complete_parser.add_argument(
        "--json",
        action="store_true",
        help="Print a JSON object with the start and end the completion replaces, whether the "
        "list is incomplete, and the items with their label, kind, detail, SQL type and "
        "nullability, instead of one item per line",
    )

    # --- breaking ---
    breaking_parser = subparsers.add_parser(
        "breaking",
        help="Check the queries and schema of two revisions across, as during a rolling "
        "deploy: sqlty breaking --against origin/main",
    )
    breaking_parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        help="Files or directories to check, in both revisions (default: current directory)",
    )
    breaking_parser.add_argument(
        "--against",
        metavar="BASELINE",
        help="The baseline: a revision (a branch, tag, SHA or HEAD~1), a snapshot written by "
        "--save, a directory of snapshots (each checked), or a schema .sql file or directory "
        "(forward only, as it has no queries)",
    )
    breaking_parser.add_argument(
        "--save",
        metavar="PATH",
        type=Path,
        help="Write a snapshot of --head (its settings, schema and Python files) to PATH, a "
        "baseline for a later --against",
    )
    breaking_parser.add_argument(
        "--head",
        metavar="REF",
        help="The new revision (default: the working tree, uncommitted changes included)",
    )
    breaking_parser.add_argument(
        "--direction",
        choices=["backward", "forward", "both"],
        default="both",
        help="backward: queries of --against on the schema of --head (migrate first); forward: "
        "queries of --head on the schema of --against (deploy first)",
    )
    breaking_parser.add_argument(
        "-q", "--quiet", action="store_true", help="Print nothing; only the exit code tells"
    )
    breaking_parser.add_argument(
        "--output-format",
        choices=["text", "json", "github", "sarif"],
        default="text",
        help="How findings are printed: text (the default), json (an array of objects with "
        "file, line, rule, severity, message, direction and against), github (GitHub "
        "Actions annotations, each at its severity) or sarif (a SARIF 2.1.0 log, for GitHub "
        "code scanning)",
    )

    # --- rule ---
    rule_parser = subparsers.add_parser(
        "rule",
        help="Explain a lint rule (sqlty rule stale-type), or list all rules",
    )
    rule_parser.add_argument("name", nargs="?", help="Rule name, for example select-star")

    # --- skill ---
    skill_parser = subparsers.add_parser(
        "skill",
        help="Print an Agent Skill for coding agents, or install them all (sqlty skill --install .claude/skills)",
    )
    skill_parser.add_argument(
        "name", nargs="?", default="sqlty", help="Skill to print (default: sqlty)"
    )
    skill_mode = skill_parser.add_mutually_exclusive_group()
    skill_mode.add_argument(
        "--install",
        metavar="DIR",
        type=Path,
        help="Write every skill to DIR/NAME/ instead of printing one",
    )
    skill_mode.add_argument(
        "--list", action="store_true", help="List the skills and what each is for"
    )

    # --- server ---
    subparsers.add_parser(
        "server",
        help="Run the language server on stdin and stdout, for editors (diagnostics and fixes as you type)",
    )

    # --- version ---
    subparsers.add_parser("version", help="Print version and exit")
    return parser


def _rule_severity(severity: str, rule: str) -> tuple[str, str]:
    return rule, severity


def _dialect(name: str) -> str:
    """``--dialect``: a dialect name, in any case."""
    import argparse

    from sqlty._config import dialect_problem

    if problem := dialect_problem(name):
        raise argparse.ArgumentTypeError(problem)
    return name.lower()


def _add_common_args(parser: argparse.ArgumentParser) -> None:
    """Add arguments shared across rewrite/strip/check."""
    import argparse

    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        default=[Path(".")],
        help="Files or directories (default: .); each file uses its nearest sqlty.toml, pyproject.toml or Cargo.toml. "
        "A project's config or schema file stands for the whole project",
    )
    parser.add_argument(
        "--dialect",
        type=_dialect,
        metavar="DIALECT",
        help="SQL dialect: postgres, mysql, sqlite, or another sqlparser dialect such as "
        "duckdb (default: [tool.sqlty] dialect, else postgres)",
    )
    parser.add_argument(
        "--schema",
        action="append",
        help="Schema .sql file, directory or glob for every path; repeat for several "
        "(default: [tool.sqlty] schema per project)",
    )
    source = parser.add_mutually_exclusive_group()
    source.add_argument(
        "--config-file",
        type=Path,
        metavar="PATH",
        help="Use this file for every path instead of the nearest config file: [tool.sqlty] "
        "of a pyproject.toml, [package.metadata.sqlty] or [workspace.metadata.sqlty] of a Cargo.toml, "
        "or top-level keys of another file such as sqlty.toml; its relative paths are relative to "
        "the current directory. Env: SQLTY_CONFIG_FILE",
    )
    source.add_argument(
        "--isolated",
        action="store_true",
        help="Ignore every configuration file; use the defaults and command-line flags",
    )
    parser.add_argument(
        "-c",
        "--config",
        action="append",
        metavar="KEY=VALUE",
        help="Override a configuration key with a TOML assignment, for example "
        "`-c 'dialect = \"mysql\"'` or `-c 'rules.select-star = \"ignore\"'`; repeat for "
        "several. Paths are relative to the current directory",
    )
    for severity in ("error", "warn", "ignore"):
        parser.add_argument(
            f"--{severity}",
            action="append",
            dest="rule_severity",
            metavar="RULE",
            type=functools.partial(_rule_severity, severity),
            help=f"Treat RULE as severity {severity}; repeat for several, the last flag for a rule wins",
        )
    parser.add_argument(
        "--exclude",
        action="append",
        metavar="GLOB",
        help="Skip this glob while walking directories, replacing [tool.sqlty] exclude and the "
        "default excludes; repeat for several. Relative to the current directory",
    )
    parser.add_argument(
        "--extend-exclude",
        action="append",
        metavar="GLOB",
        help="Skip this glob in addition to the other excludes; repeat for several. Relative to "
        "the current directory",
    )
    parser.add_argument(
        "--force-exclude",
        action="store_true",
        help="Apply the excludes to files and directories named on the command line too",
    )
    parser.add_argument(
        "--respect-gitignore",
        action=argparse.BooleanOptionalAction,
        default=None,
        help="Skip the files .gitignore ignores while walking directories (default: "
        "[tool.sqlty] respect-gitignore, else true)",
    )
    parser.add_argument(
        "-v",
        "--verbose",
        action="count",
        default=0,
        help="Print each file that rewrite, strip or check --fix changes, and with check the "
        "types of each named query of a query file",
    )
    parser.add_argument("-q", "--quiet", action="store_true", help="Suppress non-error output")


def _location(value: str) -> tuple[Path, int, int]:
    """``FILE:LINE:COL`` of ``sqlty complete``."""
    import argparse

    path, _, rest = value.rpartition(":")
    path, _, line = path.rpartition(":")
    if not (path and line.isdigit() and rest.isdigit()):
        raise argparse.ArgumentTypeError(f"expected FILE:LINE:COL, got {value!r}")
    return Path(path), int(line), int(rest)


def _workers() -> int:
    """Threads checking files at once: every processor."""
    return os.process_cpu_count() or 1


# --- Command implementations ---


def read_source(path: Path) -> str | None:
    """The text of the Python file ``path``, or ``None``, logged, when it is not UTF-8.

    sqlty reads sources as UTF-8, Python's default; a file in another encoding is
    skipped rather than stopping the run. A byte order mark is left out and line endings
    read as ``\n``.
    """
    try:
        return path.read_text(encoding="utf-8-sig")
    except UnicodeDecodeError as exc:
        logger.error("%s: not UTF-8 (%s at byte %d); skipped", path, exc.reason, exc.start)
        return None


def _hard_exit(code: int) -> NoReturn:
    """End the process without freeing what the run built, as mypy's ``hard_exit`` does:
    freeing the index of a large project takes about as long as checking it."""
    sys.stdout.flush()
    sys.stderr.flush()
    os._exit(code)


def json_diagnostic(diag: Diagnostic, parsed: ParsedSource) -> dict[str, object]:
    """``diag`` for ``--output-format json``: its offsets as 1-based lines and columns."""

    def at(offset: int) -> dict[str, int]:
        line, column = parsed.position(offset)
        return {"line": line, "column": column}

    fix = [
        {"start": at(start), "end": at(end), "replacement": text}
        for start, end, text in diag["fix"]
    ]
    return {**diag, "start": at(diag["start"]), "end": at(diag["end"]), "fix": fix}


def github_annotation(path: Path, diag: Diagnostic) -> str:
    """``diag`` as a GitHub Actions workflow command, which annotates the line in the diff."""

    def escape(text: str, *, property: bool = False) -> str:
        text = text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
        return text.replace(":", "%3A").replace(",", "%2C") if property else text

    level = "error" if diag["severity"] == "error" else "warning"
    where = f"file={escape(str(path), property=True)}"
    if "line" in diag:
        where += f",line={diag['line']}"
    return f"::{level} {where},title={escape('sqlty ' + diag['rule'], property=True)}::{escape(diag['message'])}"


def _cmd_complete(args: argparse.Namespace) -> None:
    """Complete command: the names that can be typed at a cursor in a query string.

    The file is read with the settings ``check`` gives it, and the helpers of ``paths``
    (default: its directory) are indexed, so a configured or imported helper's query
    completes too.
    """
    from sqlty._config import ConfigError
    from sqlty.core import complete_source

    path, line, column = args.location
    if not path.is_file():
        raise ConfigError(f"{path}: no such file")
    args.paths = [path, *(args.paths or [path.parent])]
    project = Project(args)
    path = next((file for file in project.files if file.resolve() == path.resolve()), path)
    if path not in project.sources:
        raise ConfigError(f"{path}: not a Python file sqlty reads")
    settings, parsed = project.settings[path], project.sources[path]
    found = complete_source(
        parsed,
        line,
        column,
        dialect=settings.dialect,
        schema_sql=settings.schema,
        import_style=settings.import_style,
        helpers=project.helpers[path],
    )
    if args.json:

        def at(offset: int) -> dict[str, int]:
            line, column = parsed.position(offset)
            return {"line": line, "column": column}

        result = (
            None
            if found is None
            else {
                "start": at(found.start),
                "end": at(found.end),
                "incomplete": found.incomplete,
                "items": [item._asdict() for item in found.items],
            }
        )
        print(json.dumps(result, indent=2))
    elif found is not None:
        for item in found.items:
            print("\t".join([item.label, item.kind, item.detail or "", item.sql_type or ""]))


def _cmd_version() -> None:
    """Print version."""
    from sqlty import __version__

    print(f"sqlty {__version__}")


# --- File helpers ---


def _collect_python_files(paths: list[Path], source: ConfigSource | None = None) -> list[Path]:
    """Collect .py files from paths (files and directories).

    Files named on the command line are collected unless ``source`` has
    ``force_exclude`` and excludes them. Walking a directory skips the excludes
    ``source`` gives it (by default those of the nearest config, or ``.git``,
    ``.venv``, ``node_modules``, ...). A path that does not exist raises ``ConfigError``.
    """
    from sqlty._config import ConfigError

    source = source or ConfigSource()
    files: list[Path] = []
    for path in paths:
        if path.exists() and source.force_exclude and source.is_excluded(path):
            continue
        if path.is_file():
            if path.suffix == ".py":
                files.append(path)
        elif path.is_dir():
            files.extend(source.walk(path))
        else:
            raise ConfigError(f"path does not exist: {path}")
    return list(dict.fromkeys(files))


def _paths(spec: str | list[str] | tuple[str, ...]) -> list[str]:
    return [spec] if isinstance(spec, str) else list(spec)


def _collect_query_files(
    paths: list[Path], python_files: list[Path], resolver: SettingsResolver
) -> list[Path]:
    """The ``.sql`` files of named queries under ``paths``: those the ``queries`` key matches.

    The projects are those of ``paths`` and of the Python files found under them; a file
    the ``queries`` globs don't match, a migration say, is never read as queries.
    """
    from sqlty._config import expand_schema

    directories = [path if path.is_dir() else path.parent for path in paths]
    directories += map(Path, dict.fromkeys(os.path.dirname(path) for path in python_files))
    roots = [path.resolve() for path in paths]
    found: dict[Path, None] = {}
    for directory in dict.fromkeys(directories):
        project = resolver.for_file(directory / "_")
        for settings in [project, *(database for _, database in project.databases)]:
            if not settings.queries or settings.config_dir is None:
                continue
            for file in expand_schema(list(settings.queries), settings.config_dir):
                resolved = file.resolve()
                if file.suffix == ".sql" and any(
                    resolved == root or resolved.is_relative_to(root) for root in roots
                ):
                    found[_relative(file)] = None
    return sorted(found)


def _relative(path: Path) -> Path:
    cwd = Path.cwd().resolve()
    resolved = path.resolve()
    return resolved.relative_to(cwd) if resolved.is_relative_to(cwd) else path


def _project_paths(paths: list[Path], source: ConfigSource) -> list[Path]:
    """``paths`` with a project's config or schema file replaced by its directory.

    A schema or config change can break any query of the project, so a pre-commit hook
    that passes only the changed files still checks them all.
    """
    from sqlty._config import expand_schema

    projects = []
    for path in paths:
        if path.is_file() and path.suffix != ".py":
            config, config_dir, label = source.load(path.parent.resolve())
            if config_dir is not None:
                schema = expand_schema(config.get("schema", []), config_dir)
                owned = {Path(label).resolve(), *(file.resolve() for file in schema)}
                if path.resolve() in owned:
                    cwd = Path.cwd().resolve()
                    path = (
                        config_dir.relative_to(cwd)
                        if config_dir.is_relative_to(cwd)
                        else config_dir
                    )
        projects.append(path)
    return list(dict.fromkeys(projects))


def _config_file(args: argparse.Namespace) -> Path | None:
    """``--config-file``, else ``SQLTY_CONFIG_FILE`` unless ``--isolated`` (like ty's ``TY_CONFIG_FILE``)."""
    if getattr(args, "config_file", None) is not None:
        return Path(args.config_file)
    env = os.environ.get("SQLTY_CONFIG_FILE")
    return Path(env) if env and not getattr(args, "isolated", False) else None


class ConfigSource:
    """Each directory's ``[tool.sqlty]`` after ``--config-file``, ``--isolated``,
    ``-c/--config``, ``--error/--warn/--ignore`` and the exclude flags.

    By default it is the nearest ``sqlty.toml``, or ``pyproject.toml`` or ``Cargo.toml`` with a table. ``config_file``
    fixes one file for every directory, with relative paths resolved against
    the current directory (like ruff); ``isolated`` uses no file at all.
    ``overrides`` are TOML assignments applied on top, then ``rule_severity``
    ``(rule, severity)`` pairs in order.

    Example:
        >>> ConfigSource(isolated=True, extend_exclude=["gen"]).exclusions(Path("."))[-1][1]
        ('gen',)
    """

    def __init__(
        self,
        config_file: Path | None = None,
        *,
        isolated: bool = False,
        overrides: list[str] | None = None,
        rule_severity: list[tuple[str, str]] | None = None,
        exclude: list[str] | None = None,
        extend_exclude: list[str] | None = None,
        force_exclude: bool = False,
        respect_gitignore: bool | None = None,
    ) -> None:
        from sqlty import _core
        from sqlty._config import load_config_file, parse_overrides, validate_rule_severity

        self._isolated = isolated
        self._fixed = None if config_file is None else load_config_file(config_file)
        self._label = str(config_file) if config_file is not None else None
        self._cwd = Path.cwd().resolve()
        inline = parse_overrides(overrides or [])
        self.schema = inline.pop("schema", None)
        if exclude is None and "exclude" in inline:
            exclude = inline["exclude"]
        inline.pop("exclude", None)
        extend_exclude = [*inline.pop("extend_exclude", []), *(extend_exclude or ())]
        self._overrides = inline
        self._rule_severity = validate_rule_severity(rule_severity or [])
        self.force_exclude = force_exclude
        respect_gitignore = inline.pop("respect_gitignore", respect_gitignore)
        fixed: dict[str, object] | None = None
        if self._fixed is not None:
            fixed = {
                key.replace("_", "-"): self._fixed[key]
                for key in ("exclude", "extend_exclude", "respect_gitignore")
                if key in self._fixed
            }
        self._discovery = _core.Discovery(
            str(self._cwd),
            isolated=isolated,
            fixed=fixed,
            exclude=None if exclude is None else list(exclude),
            extend_exclude=list(extend_exclude),
            respect_gitignore=respect_gitignore,
        )

    @classmethod
    def from_args(cls, args: argparse.Namespace) -> ConfigSource:
        return cls(
            _config_file(args),
            isolated=getattr(args, "isolated", False),
            overrides=getattr(args, "config", None),
            rule_severity=getattr(args, "rule_severity", None),
            exclude=getattr(args, "exclude", None),
            extend_exclude=getattr(args, "extend_exclude", None),
            force_exclude=getattr(args, "force_exclude", False),
            respect_gitignore=getattr(args, "respect_gitignore", None),
        )

    def load(self, directory: Path) -> tuple[SQLTyConfig, Path | None, str]:
        """Config governing ``directory``, the directory its relative paths start from, and its
        file name. Inline overrides and rule flags are applied; ``schema`` and the excludes
        given inline are not (``self.schema``, ``exclusions``)."""
        from sqlty._config import find_config

        if self._isolated:
            config: SQLTyConfig = {}
            config_dir, label = None, "--isolated"
        elif self._fixed is not None:
            config, config_dir, label = self._fixed, self._cwd, str(self._label)
        else:
            config, path = find_config(directory)
            config_dir = path.parent if path else None
            label = str(path) if path else "pyproject.toml"
        if self._overrides or self._rule_severity:
            rules = {**config.get("rules", {}), **self._overrides.get("rules", {})}
            rules.update(self._rule_severity)
            config = {**config, **self._overrides}
            if rules:
                config["rules"] = rules
        return config, config_dir, label

    @property
    def cli_rules(self) -> dict[str, str]:
        """Rule severities from ``-c rules.X`` and ``--error/--warn/--ignore``, which win over every file."""
        return {**self._overrides.get("rules", {}), **dict(self._rule_severity)}

    def origin(self, key: str, label: str) -> str:
        """Where ``key`` of the config ``load`` returned with ``label`` came from, for error messages."""
        return "--config" if key in self._overrides else label

    def exclusions(self, directory: Path) -> list[tuple[Path, tuple[str, ...]]]:
        """``(base directory, globs)`` pairs excluding entries of ``directory``; empty sets left out."""
        found = _discovered(self._discovery.exclusions, directory)
        return [(base, tuple(patterns)) for base, patterns in found]

    def is_excluded(self, path: Path) -> bool:
        """Whether ``path`` or one of its parents up to a glob's base directory is excluded."""
        return _discovered(self._discovery.is_excluded, path)

    def route(
        self, routing: dict[str, object], base: Path, path: Path
    ) -> tuple[str | None, dict[str, str]]:
        """The database ``path`` belongs to and the rules its overrides set; see ``_routing``."""
        return self._discovery.route(routing, str(base), str(path))

    def walk(self, root: Path) -> list[Path]:
        """The ``.py`` files under ``root``, sorted, without excluded directories and files."""
        return _discovered(self._discovery.walk, root)


def _discovered[T](method: Callable[[str], T], path: Path) -> T:
    from sqlty._config import ConfigError

    try:
        return method(str(path))
    except ValueError as exc:
        raise ConfigError(str(exc)) from None


def _checked_roots(paths: Iterable[Path], files: Iterable[Path]) -> list[Path]:
    """The checked directories, the parents of checked files and the package roots of ``files``.

    Example:
        >>> [root.name for root in _checked_roots([Path("/missing/q.py")], [])]
        ['missing']
    """
    from sqlty import _core

    return _core.checked_roots(list(paths), list(files), Path.cwd())


def _module_names(path: Path, roots: list[Path]) -> list[str]:
    """Dotted names the resolved ``path`` can be imported by, the likeliest first.

    The package path (up through the ``__init__.py`` directories) comes first,
    then the path from each checked directory, with and without its own name.
    ``pkg/__init__.py`` is ``pkg``.

    Example:
        >>> _module_names(Path("missing/pkg/mod.py"), [])
        ['mod']
    """
    return _modules_names([path], roots)[0]


def _modules_names(paths: list[Path], roots: list[Path]) -> list[list[str]]:
    """``_module_names`` of each of ``paths``."""
    from sqlty import _core

    return _core.module_names(paths, roots, Path.cwd())


@dataclass(frozen=True)
class Settings:
    """The nearest configuration file (``sqlty.toml``, ``pyproject.toml``, ``Cargo.toml``), with CLI flags on top."""

    dialect: str = "postgres"
    schema: Schema | None = None
    schema_sql: str | None = None
    types: dict[str, str] | None = None
    import_style: str | None = None
    untyped: bool = False
    json_types: str = "typing.Any"
    param_types: bool = True
    closed_params: bool = False
    rules: dict[str, str] | None = None
    query_variants: int | None = None
    preview: bool = False
    coerce_row_types: frozenset[str] = frozenset()
    functions: tuple[tuple[str, str, str | None], ...] = ()
    # The functions each named database lists, read against it in any file.
    database_functions: tuple[tuple[str, str, str | None, str], ...] = ()
    format_command: tuple[str, ...] = ()
    overrides: tuple[RuleOverride, ...] = ()
    config_dir: Path | None = None
    queries: tuple[str, ...] = ()
    query_format: str = "auto"
    databases: tuple[tuple[str, Settings], ...] = ()
    # What picks the database and overrides of a file: ``queries``, ``databases``, ``overrides``.
    routing: dict[str, object] | None = None

    def names_query_file(self, path: Path) -> bool:
        """Whether ``queries`` names ``path``, as a file, a directory holding it or a glob."""
        from sqlty import _core

        return self.config_dir is not None and _core.names_query_file(
            str(path), str(self.config_dir), list(self.queries)
        )

    @property
    def schema_key(self) -> SchemaKey:
        """What ``schema`` is loaded from: two settings with one key check queries alike."""
        return self.schema_sql, self.dialect, tuple(sorted((self.types or {}).items()))


class SettingsResolver:
    """Resolve settings per file, so each project directory uses its own config.

    Example:
        >>> import argparse
        >>> args = argparse.Namespace(dialect="sqlite", schema=None, untyped=None)
        >>> SettingsResolver(args).for_file(Path("missing/dir/q.py")).dialect
        'sqlite'
    """

    def __init__(self, args: argparse.Namespace) -> None:
        self._args = args
        self.source = ConfigSource.from_args(args)
        schema = args.schema or self.source.schema
        self._schema_sql = _load_schema(schema, Path.cwd()) if schema else None
        self._by_dir: dict[Path, Settings] = {}
        self._resolved: dict[Path, Path] = {}
        self._schemas: dict[tuple[str, str, tuple[tuple[str, str], ...]], Schema] = {}

    def _schema(self, schema_sql: str, dialect: str, types: dict[str, str] | None = None) -> Schema:
        """``schema_sql`` parsed once per run, warning about the objects it could not parse."""
        from sqlty._config import ConfigError
        from sqlty.core import checks_names, load_schema, schema_skipped_objects

        key = (schema_sql, dialect, tuple(sorted((types or {}).items())))
        if (schema := self._schemas.get(key)) is None:
            try:
                schema = self._schemas[key] = load_schema(schema_sql, dialect=dialect, types=types)
            except ValueError as exc:
                raise ConfigError(f"[tool.sqlty.types]: {exc}") from None
            if not checks_names(dialect):
                logger.warning("Table and column names are not checked for %s", dialect)
            for kind, name in schema_skipped_objects(schema, dialect=dialect):
                if kind == "statement":
                    logger.warning("Schema statement could not be parsed and is ignored: %s", name)
                elif kind == "limit":
                    logger.warning("Schema is too large to load; ignored: %s", name)
                else:
                    logger.warning(
                        "Schema %s %s could not be parsed; its columns are unknown", kind, name
                    )
        return schema

    def resolve(self, path: Path) -> Path:
        """``path`` resolved; each directory is resolved once, as its files share it."""
        if path.is_symlink():
            return path.resolve()
        return self._directory(path.parent) / path.name

    def _directory(self, directory: Path) -> Path:
        if (resolved := self._resolved.get(directory)) is None:
            resolved = self._resolved[directory] = directory.resolve()
        return resolved

    def for_file(self, path: Path) -> Settings:
        """The settings of ``path``: those of the first of its project's ``databases`` whose
        ``queries`` name it or whose ``include`` matches it, else the project's own."""
        directory = self._directory(path.parent)
        if directory not in self._by_dir:
            self._by_dir[directory] = self._resolve(directory)
        settings = self._by_dir[directory]
        if settings.routing is None or settings.config_dir is None:
            return settings
        database, rules = self.source.route(settings.routing, settings.config_dir, path)
        if database is not None:
            settings = dict(settings.databases)[database]
        if not settings.overrides:
            return settings
        return replace(settings, rules=(settings.rules or {}) | rules | self.source.cli_rules)

    def _resolve(self, directory: Path) -> Settings:

        config, config_dir, label = self.source.load(directory)
        schema_sql = self._schema_sql
        if schema_sql is None and "schema" in config and config_dir is not None:
            schema_sql = _load_schema(config["schema"], config_dir)
        settings = self._settings(config, config_dir, label, schema_sql, self._args.dialect)
        databases = []
        if config.get("databases") or config.get("overrides"):
            settings = replace(settings, routing=_routing(config))
        for name, database in config.get("databases", {}).items():
            merged = cast("SQLTyConfig", {**config, **database})
            merged.pop("databases")
            for key in ("rules", "types"):
                if key in config and key in database:
                    merged[key] = {**config[key], **database[key]}
            own_schema = schema_sql
            if "schema" in database and config_dir is not None:
                own_schema = _load_schema(database["schema"], config_dir)
            databases.append(
                (
                    name,
                    self._settings(
                        merged,
                        config_dir,
                        f"{label} [tool.sqlty.databases.{name}]",
                        own_schema,
                        None if "dialect" in database else self._args.dialect,
                    ),
                )
            )
        if not databases:
            return settings
        from sqlty.core import load_schema

        named = {"default": settings} | dict(databases)
        plain = {
            name: db.schema or load_schema(None, dialect=db.dialect) for name, db in named.items()
        }
        database_functions = tuple(
            (f["import_path"], f["query_param"], f.get("return_type_param"), name)
            for name, database in config["databases"].items()
            for f in database.get("functions", [])
        )
        routed = {
            name: replace(
                db,
                database_functions=database_functions,
                schema=load_schema(
                    db.schema_sql,
                    dialect=db.dialect,
                    types=db.types,
                    name=name,
                    databases={other: plain[other] for other in named if other != name},
                ),
            )
            for name, db in named.items()
        }
        return replace(
            routed["default"], databases=tuple((name, routed[name]) for name, _ in databases)
        )

    def _settings(
        self,
        config: SQLTyConfig,
        config_dir: Path | None,
        label: str,
        schema_sql: str | None,
        dialect_flag: str | None,
    ) -> Settings:
        from sqlty._config import ConfigError
        from sqlty.core import strip_source

        args = self._args
        untyped = getattr(args, "untyped", None) or config.get("untyped", "skip")
        if untyped not in ("skip", "sentinel"):
            raise ConfigError(
                f"{self.source.origin('untyped', label)}: untyped must be 'skip' or 'sentinel'"
            )
        json_types = config.get("json_types", "typing.Any")
        if json_types not in ("typing.Any", "typing.TypedDict"):
            raise ConfigError(
                f"{self.source.origin('json_types', label)}: json-types must be 'typing.Any' or 'typing.TypedDict'"
            )
        import_style = config.get("import_style")
        try:
            strip_source("", import_style=import_style)
        except ValueError as exc:
            raise ConfigError(
                f"{self.source.origin('import_style', label)}: {exc}; "
                "use 'from sqlty import SQL [as Q]' or 'import sqlty [as sy]'"
            ) from None
        from sqlty._config import (
            MAX_QUERY_VARIANTS,
            driver_types,
            requires_closed,
            requires_python,
        )

        dialect = dialect_flag or config.get("dialect", "postgres")
        types = driver_types(config.get("driver"), dialect, config.get("types"))
        return Settings(
            dialect=dialect,
            schema=None if schema_sql is None else self._schema(schema_sql, dialect, types),
            schema_sql=schema_sql,
            types=types,
            import_style=import_style,
            untyped=untyped == "sentinel",
            json_types=json_types,
            param_types=bool(config.get("param_types", True)),
            closed_params=requires_closed(requires_python(config_dir)),
            rules=config.get("rules"),
            query_variants=config.get("max_query_variants", MAX_QUERY_VARIANTS)
            if config.get("preview")
            else None,
            preview=bool(config.get("preview")),
            coerce_row_types=frozenset(config.get("coerce_row_types", ())),
            functions=tuple(
                (f["import_path"], f["query_param"], f.get("return_type_param"))
                for f in config.get("functions", [])
            ),
            format_command=tuple(config.get("format_command", ())),
            overrides=tuple(config.get("overrides", ())),
            config_dir=config_dir,
            queries=tuple(_paths(config.get("queries", ()))),
            query_format=config.get("query_format", "auto"),
        )


class Project:
    """The Python files ``check`` reads, with their sources, settings and helpers, by path.

    ``update`` replaces the text of one file, as an editor does with an unsaved buffer.
    """

    def __init__(self, args: argparse.Namespace) -> None:
        from sqlty._source import ParsedSource

        self.resolver = SettingsResolver(args)
        self.paths: list[Path] = _project_paths(args.paths, self.resolver.source)
        files = _collect_python_files(self.paths, self.resolver.source)
        texts = {path: read_source(path) for path in files}
        self.unreadable = [path for path, text in texts.items() if text is None]
        self.files = [path for path in files if texts[path] is not None]
        self.resolved = {path: self.resolver.resolve(path) for path in self.files}
        resolved = [self.resolved[path] for path in self.files]
        self.modules = dict(zip(self.files, _modules_names(resolved, self.paths), strict=True))
        self.sources = {
            path: ParsedSource(text, self._module(path))
            for path in self.files
            if (text := texts[path]) is not None
        }
        self._fill_index()
        self.settings = {path: self.resolver.for_file(path) for path in self.files}
        self.query_files = [
            path
            for path in _collect_query_files(self.paths, self.files, self.resolver)
            if path not in self.settings
        ]
        self._configure_helpers()

    def is_query_file(self, path: Path) -> bool:
        """Whether the ``queries`` key of ``path``'s project names it, as a file, a directory
        holding it or a glob; ``path`` need not exist, as an unsaved buffer's may not."""
        return self.resolver.for_file(path).names_query_file(path)

    def check_queries(self, path: Path, text: str) -> list[Diagnostic]:
        from sqlty._queries import check_query_file

        settings = self.resolver.for_file(path)
        return check_query_file(
            text,
            name=path.stem,
            query_format=settings.query_format,
            dialect=settings.dialect,
            schema_sql=settings.schema,
            rules=settings.rules,
        )

    def update(self, path: Path, text: str) -> None:
        from sqlty._source import ParsedSource

        old = self.sources.get(path)
        if old is not None and old.text == text:
            return
        if old is None:
            self.files.append(path)
            self.resolved[path] = self.resolver.resolve(path)
            self.modules[path] = _module_names(self.resolved[path], self.paths)
            self.settings[path] = self.resolver.for_file(path)
        parsed = self.sources[path] = ParsedSource(text, self._module(path))
        if old is None or old.may_define_helpers or parsed.may_define_helpers:
            self._index()
            return
        for name in self.modules[path]:
            self.index.add(name, parsed)

    def check(self, path: Path, row_usage: RowUsage | None = None) -> list[Diagnostic]:
        from sqlty.core import check_source

        settings = self.settings[path]
        return check_source(
            self.sources[path],
            dialect=settings.dialect,
            schema_sql=settings.schema,
            rules=settings.rules,
            import_style=settings.import_style,
            untyped=settings.untyped,
            helpers=self.helpers[path],
            row_usage=row_usage,
            query_variants=settings.query_variants,
            coerce_row_types=settings.coerce_row_types,
        )

    def fix(self, path: Path) -> str:
        from sqlty.core import fix_source

        settings = self.settings[path]
        return fix_source(
            self.sources[path],
            dialect=settings.dialect,
            schema_sql=settings.schema,
            import_style=settings.import_style,
            untyped=settings.untyped,
            rules=settings.rules,
            json_types=settings.json_types,
            param_types=settings.param_types,
            closed_params=settings.closed_params,
            helpers=self.helpers[path],
        )

    def _module(self, path: Path) -> str:
        return self.modules[path][0] + (".__init__" if path.name == "__init__.py" else "")

    def _index(self) -> None:
        self._fill_index()
        self._configure_helpers()

    def _fill_index(self) -> None:
        """The index of the sources; Rust builds its own on a thread of its own meanwhile."""
        import threading

        from sqlty._helpers import HelperIndex

        self.index = HelperIndex()
        for path, parsed in self.sources.items():
            for name in self.modules[path]:
                self.index.add(name, parsed)
        threading.Thread(target=self.index.rust, daemon=True).start()

    def _configure_helpers(self) -> None:
        from sqlty._helpers import configured_helpers

        checked = _checked_roots(self.paths, self.resolved.values())
        helpers_for = configured_helpers(self.index, checked)
        self.helpers = {path: helpers_for(self.settings[path]) for path in self.files}


def _routing(config: SQLTyConfig) -> dict[str, object]:
    """The keys of ``config`` that pick the database and overrides of a file."""
    databases = config.get("databases", {})
    return {
        "queries": _paths(config.get("queries", ())),
        "databases": {
            name: {
                key: database[key] for key in ("queries", "include", "exclude") if key in database
            }
            for name, database in databases.items()
        },
        "overrides": list(config.get("overrides", ())),
    }


def _load_schema(spec: SchemaSpec, base: Path) -> str | None:
    """Concatenate the schema files named by ``spec``; None if none can be read."""
    from sqlty._config import expand_schema

    parts: list[str] = []
    for path in expand_schema(spec, base):
        if path.is_file():
            parts.append(path.read_text(encoding="utf-8"))
        else:
            logger.warning("Schema file not found: %s", path)
    return "\n".join(parts) if parts else None
