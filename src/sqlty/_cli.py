"""CLI entry point — ruff-style subcommand router.

Commands:
    sqlty rewrite [OPTIONS] [PATH...]   Rewrite source files
    sqlty strip [OPTIONS] [PATH...]     Strip SQL[T](...) wrappers
    sqlty check [OPTIONS] [PATH...]     Lint SQL type annotations
    sqlty rule [NAME]                   Explain a lint rule, or list them
    sqlty version                       Print version
"""

from __future__ import annotations

import codecs
import fnmatch
import functools
import gc
import glob
import itertools
import json
import logging
import os
import re
import sys
from dataclasses import dataclass, replace
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import argparse
    from collections import Counter
    from collections.abc import Callable, Iterable, Iterator

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
    args = parser.parse_args(sys.argv[1:] if argv is None else argv)

    if args.version:
        _cmd_version()
        return

    if args.command is None:
        parser.print_help()
        sys.exit(0)

    from sqlty._config import ConfigError

    try:
        match args.command:
            case "rewrite":
                _cmd_rewrite(args)
            case "strip":
                _cmd_strip(args)
            case "check":
                _cmd_check(args)
            case "breaking":
                from sqlty._breaking import main as breaking

                breaking(args)
            case "rule":
                _cmd_rule(args.name)
            case "skill":
                _cmd_skill(args.name, args.install, args.list)
            case "version":
                _cmd_version()
            case _:
                parser.print_help()
    except ConfigError as exc:
        print(f"sqlty: error: {exc}", file=sys.stderr)
        sys.exit(EXIT_ERROR)
    except Exception:
        import traceback

        traceback.print_exc()
        print("sqlty: internal error; please report it with the traceback above", file=sys.stderr)
        sys.exit(EXIT_ERROR)


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
        choices=["text", "json", "github"],
        default="text",
        help="How diagnostics are printed: text (file:line: [rule] message, the default), json "
        "(an array of objects with file, line, rule, severity and message) or github (GitHub "
        "Actions annotations). Summaries, --statistics and --explain are text only",
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
        choices=["text", "json", "github"],
        default="text",
        help="How findings are printed: text (the default), json (an array of objects with "
        "file, line, rule, severity, message, direction and against) or github (GitHub "
        "Actions annotations, each at its severity)",
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
        help="Files or directories (default: .); each file uses its nearest pyproject.toml",
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
        help="Use this file for every path instead of the nearest pyproject.toml: [tool.sqlty] "
        "of a pyproject.toml, or top-level keys of another file such as sqlty.toml; its "
        "relative paths are relative to the current directory. Env: SQLTY_CONFIG_FILE",
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
        help="Print each file that rewrite, strip or check --fix changes",
    )
    parser.add_argument("-q", "--quiet", action="store_true", help="Suppress non-error output")
    parser.add_argument(
        "-j",
        "--jobs",
        type=_jobs,
        metavar="N",
        help="Files to work on at once; 1 works on one at a time. Output is the same in any "
        "case (default: the number of processors)",
    )


def _jobs(value: str) -> int:
    import argparse

    if not value.isdigit() or int(value) < 1:
        raise argparse.ArgumentTypeError(f"expected a positive integer, got {value!r}")
    return int(value)


def _map[T](args: argparse.Namespace, work: Callable[[Path], T], files: list[Path]) -> Iterator[T]:
    """``work`` on every file, its results in the order of ``files``.

    The engine releases the GIL, so threads check files at once. Whatever prints or writes
    stays with the caller, which reads the results in order.
    """
    jobs = min(args.jobs or os.process_cpu_count() or 1, len(files))
    if jobs <= 1:
        yield from map(work, files)
        return
    from concurrent.futures import ThreadPoolExecutor

    # A task per file costs more than most files take; four batches a thread keep them balanced.
    batches = itertools.batched(files, max(1, min(64, len(files) // (jobs * 4))), strict=False)
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        for batch in pool.map(
            lambda batch: [work(path) for path in batch], batches, buffersize=jobs
        ):
            yield from batch


# --- Command implementations ---

_SKIPPED_STRING_RULES = ("fstring-skipped", "tstring-skipped", "unchecked-query", "dynamic-query")


def _cmd_rewrite(args: argparse.Namespace) -> None:
    """Rewrite command: transform --sql strings into SQL[T](...) calls.

    f- and t-strings starting with ``--sql`` are never rewritten; they are
    reported on stderr (``fstring-skipped``, ``tstring-skipped``) unless ``-q``.
    """
    from sqlty._lint import RULES
    from sqlty.core import check_source, rewrite_source

    def transform(path: Path, original: str, settings: Settings) -> tuple[str, list[str]]:
        rewritten = rewrite_source(
            original,
            dialect=settings.dialect,
            schema_sql=settings.schema,
            import_style=settings.import_style,
            untyped=settings.untyped,
            json_types=settings.json_types,
        )
        if args.quiet:
            return rewritten, []
        skipped_only = dict.fromkeys(RULES, "ignore") | {
            rule: (settings.rules or {}).get(rule, RULES[rule].default_severity.value)
            for rule in _SKIPPED_STRING_RULES
        }
        diagnostics = check_source(original, import_style=settings.import_style, rules=skipped_only)
        return rewritten, [
            f"{path}:{diag['line']}: [{diag['rule']}] {diag['message']}" for diag in diagnostics
        ]

    _transform_files(args, transform, "rewrite", "Rewrote", "rewritten")


def _cmd_strip(args: argparse.Namespace) -> None:
    """Strip command: remove SQL[T](...) wrappers."""
    from sqlty.core import strip_source

    def transform(path: Path, original: str, settings: Settings) -> tuple[str, list[str]]:
        return strip_source(original, import_style=settings.import_style), []

    _transform_files(args, transform, "strip", "Stripped", "stripped")


def _transform_files(
    args: argparse.Namespace,
    transform: Callable[[Path, str, Settings], tuple[str, list[str]]],
    verb: str,
    done: str,
    participle: str,
) -> None:
    """Apply ``transform`` to every collected file for ``rewrite`` and ``strip``.

    Writes the result back. With ``--check`` or ``--diff`` it leaves the files
    alone and exits 1 when any would change.
    """
    resolver = SettingsResolver(args)
    files = _collect_python_files(args.paths, resolver.source)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    texts = {path: read_source(path) for path in files}
    unreadable = sum(text is None for text in texts.values())
    sources = {path: text for path, text in texts.items() if text is not None}
    settings = {path: resolver.for_file(path) for path in sources}
    readable = list(sources)

    def work(path: Path) -> tuple[str, list[str]]:
        return transform(path, sources[path], settings[path])

    changed = 0
    for path, (result, warnings) in zip(readable, _map(args, work, readable), strict=True):
        for warning in warnings:
            print(warning, file=sys.stderr)
        original = sources[path]
        if result == original:
            continue

        changed += 1
        if args.check:
            if not args.quiet:
                print(f"Would {verb} {path}")
        elif args.diff:
            _print_diff(path, original, result)
        else:
            write_source(path, result)
            if verb == "rewrite":
                _format(path, settings[path].format_command)
            if args.verbose:
                print(f"{done} {path}")

    if args.check or args.diff:
        if changed > 0:
            if args.check and not args.quiet:
                print(f"{changed} file(s) would be {participle}")
            sys.exit(EXIT_FOUND)
        elif args.check and not args.quiet and not unreadable:
            print("All files up to date")
    if unreadable:
        sys.exit(EXIT_ERROR)


def read_source(path: Path) -> str | None:
    """The text of the Python file ``path``, or ``None``, logged, when it is not UTF-8.

    sqlty reads and writes sources as UTF-8, Python's default; a file in another
    encoding is skipped rather than stopping the run. A byte order mark is left out and
    line endings read as ``\n``; `write_source` restores both.
    """
    try:
        return path.read_text(encoding="utf-8-sig")
    except UnicodeDecodeError as exc:
        logger.error("%s: not UTF-8 (%s at byte %d); skipped", path, exc.reason, exc.start)
        return None


def write_source(path: Path, text: str) -> None:
    """Write ``text`` over the Python file ``path`` with its byte order mark and line endings."""
    raw = path.read_bytes()
    path.write_text(
        text,
        encoding="utf-8-sig" if raw.startswith(codecs.BOM_UTF8) else "utf-8",
        newline="\r\n" if raw.split(b"\n", 1)[0].endswith(b"\r") else "\n",
    )


def _format(path: Path, command: tuple[str, ...]) -> bool:
    """Run the configured formatter on ``path``; whether it ran and succeeded."""
    import subprocess

    if not command:
        return False
    try:
        result = subprocess.run([*command, str(path)], capture_output=True, text=True, check=False)
    except OSError as exc:
        logger.warning("format-command %s failed on %s: %s", command[0], path, exc)
        return False
    if result.returncode != 0:
        logger.warning(
            "format-command %s failed on %s: %s", command[0], path, result.stderr.strip()
        )
        return False
    return True


def _cmd_check(args: argparse.Namespace) -> None:
    """Check command: lint SQL type annotations."""
    from collections import Counter

    from sqlty._config import ConfigError
    from sqlty._helpers import configured_helpers
    from sqlty._source import ParsedSource
    from sqlty.core import HelperIndex, RowUsage, check_source, fix_source, query_statistics

    if args.error_on_warning and (args.exit_zero or args.exit_zero_on_warning):
        other = "--exit-zero" if args.exit_zero else "--exit-zero-on-warning"
        raise ConfigError(f"--error-on-warning cannot be used with {other}")
    # Parsed trees stay alive for cross-module lookups: collect young objects only, as mypy does.
    gc.set_threshold(200_000, 30, 30)
    resolver = SettingsResolver(args)
    files = _collect_python_files(args.paths, resolver.source)

    if not files and not args.quiet:
        logger.warning("No Python files found")
        return

    texts = {path: read_source(path) for path in files}
    unreadable = [path for path, text in texts.items() if text is None]
    files = [path for path in files if texts[path] is not None]
    modules = {path: _module_names(resolver.resolve(path), args.paths) for path in files}
    sources = {
        path: ParsedSource(
            source,
            modules[path][0] + (".__init__" if path.name == "__init__.py" else ""),
        )
        for path in files
        if (source := texts[path]) is not None
    }
    helpers = HelperIndex()
    for path, parsed in sources.items():
        for name in modules[path]:
            helpers.add(name, parsed)
    helpers_for = configured_helpers(
        helpers, _checked_roots(args.paths, [resolver.resolve(path) for path in files])
    )

    total_errors = 0
    total_warnings = 0
    total_fixed = 0
    statistics: Counter[str] = Counter()
    # A row type shared by files is judged by all of its queries: report once every file is read.
    row_usage = RowUsage()
    reports: list[tuple[Path, Diagnostic, ParsedSource]] = []

    settings = {path: resolver.for_file(path) for path in files}
    file_helpers = {path: helpers_for(settings[path]) for path in files}

    def fix(path: Path) -> str:
        return fix_source(
            sources[path],
            dialect=settings[path].dialect,
            schema_sql=settings[path].schema,
            import_style=settings[path].import_style,
            untyped=settings[path].untyped,
            rules=settings[path].rules,
            json_types=settings[path].json_types,
            helpers=file_helpers[path],
        )

    if args.fix:
        for path, fixed in zip(files, _map(args, fix, files), strict=True):
            if fixed == sources[path].text:
                continue
            write_source(path, fixed)
            if _format(path, settings[path].format_command):
                fixed = path.read_text(encoding="utf-8-sig")
            sources[path] = ParsedSource(fixed, sources[path].module)
            total_fixed += 1
            if args.verbose:
                # JSON and annotations own stdout; progress goes to stderr then.
                text = args.output_format == "text"
                print(f"Fixed {path}", file=sys.stdout if text else sys.stderr)

    def check(path: Path) -> tuple[list[Diagnostic], Counter[str]]:
        diagnostics = check_source(
            sources[path],
            dialect=settings[path].dialect,
            schema_sql=settings[path].schema,
            rules=settings[path].rules,
            import_style=settings[path].import_style,
            untyped=settings[path].untyped,
            helpers=file_helpers[path],
            type_modules=settings[path].type_modules,
            row_usage=row_usage,
            query_variants=settings[path].query_variants,
        )
        if not args.statistics:
            return diagnostics, Counter()
        counts = query_statistics(
            sources[path],
            dialect=settings[path].dialect,
            schema_sql=settings[path].schema,
            import_style=settings[path].import_style,
            helpers=file_helpers[path],
            query_variants=settings[path].query_variants,
        )
        return diagnostics, counts

    for path, (diagnostics, counts) in zip(files, _map(args, check, files), strict=True):
        statistics += counts
        reports.extend((path, diag, sources[path]) for diag in diagnostics)

    kept = {id(diag) for diag in row_usage.filter([diag for _, diag, _ in reports])}
    shown: list[dict[str, object]] = []
    for path, diag, parsed in reports:
        if id(diag) not in kept:
            continue
        severity = diag["severity"]
        if severity == "error":
            total_errors += 1
        elif severity == "warn":
            total_warnings += 1
        if args.output_format == "json":
            shown.append({"file": str(path), **diag})
        elif args.output_format == "github":
            print(github_annotation(path, diag))
        elif not args.quiet:
            print(f"{path}:{diag.get('line', '')}: [{diag['rule']}] {diag['message']}")
            if args.explain and diag["rule"] in _EXPLAINED_RULES and "line" in diag:
                own = settings[path]
                for line in _explain(parsed, diag["line"], own.dialect, own.schema):
                    print(line)

    if args.output_format == "json":
        print(json.dumps(shown, indent=2))
    text = args.output_format == "text" and not args.quiet
    if total_fixed and text:
        print(f"Fixed {total_fixed} file(s)")
    if text and (total_errors or total_warnings):
        parts = []
        if total_errors:
            parts.append(f"{total_errors} error(s)")
        if total_warnings:
            parts.append(f"{total_warnings} warning(s)")
        print(f"Found {', '.join(parts)}")
    if args.statistics and text:
        print(_format_statistics(statistics))

    if not args.exit_zero and (total_errors or (args.error_on_warning and total_warnings)):
        sys.exit(EXIT_FOUND)
    if unreadable:
        sys.exit(EXIT_ERROR)


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


def _cmd_skill(name: str, install: Path | None, list_skills: bool) -> None:
    """Skill command: print one Agent Skill, list them, or write them all under ``install``."""
    from sqlty._core import skills

    files = dict(skills())
    if install is not None:
        for path, text in files.items():
            target = install / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8")
            print(f"Wrote {target}")
        return
    names = {path.split("/")[0]: text for path, text in files.items() if path.endswith("/SKILL.md")}
    if list_skills:
        width = max(map(len, names))
        for skill, text in names.items():
            description = re.search(r"^description: (.*)$", text, re.M)
            summary = description.group(1).split(". ")[0].rstrip(".") if description else ""
            print(f"{skill:<{width}}  {summary}")
        return
    if name not in names:
        logger.error("Unknown skill %r; run `sqlty skill --list` for the list", name)
        sys.exit(EXIT_ERROR)
    print(names[name], end="")


# Diagnostics about the type of a query's rows, which `--explain` traces to inference rules.
_EXPLAINED_RULES = frozenset(
    {
        "stale-type",
        "narrow-type",
        "column-type-mismatch",
        "nullable-mismatch",
        "redundant-optional",
        "literal-mismatch",
        "redundant-literal",
        "json-shape-mismatch",
    }
)


def _explain(
    parsed: ParsedSource, line: int, dialect: str, schema: str | Schema | None
) -> list[str]:
    """The inference rules behind the columns of the query on ``line``, indented."""
    from sqlty._core import query_sites
    from sqlty.core import explain_select

    sites = [(start, text) for start, kind, text in query_sites(parsed.text) if kind == "query"]
    query = next((text for start, text in reversed(sites) if start <= line), None)
    lines: list[str] = []
    for column in (
        explain_select(query, dialect=dialect, schema_sql=schema) if query else None
    ) or []:
        rules = {rule for rule in (column.type_rule, column.nullability_rule) if rule is not None}
        for rule in sorted(rules, key=lambda rule: rule.id):
            parts = [
                part
                for part, by in (
                    ("type", column.type_rule),
                    ("nullability", column.nullability_rule),
                )
                if by == rule
            ]
            lines.append(f"  {column.name}: {' and '.join(parts)} by inference rule {rule.id}")
            lines.append(f"    {rule.reasoning}")
            lines.append(f"    Reference: {rule.reference}")
    return lines


def _format_statistics(counts: Counter[str]) -> str:
    """One line of query counts; skipped strings are broken down by rule."""
    skipped = {rule: counts[rule] for rule in _SKIPPED_STRING_RULES if counts[rule]}
    detail = ", ".join(f"{rule}: {count}" for rule, count in skipped.items())
    return (
        f"Queries: {counts['typed']} typed, {counts['partially typed']} partially typed, "
        f"{counts['unparsed']} unparsed, {sum(skipped.values())} skipped"
        + (f" ({detail})" if detail else "")
    )


def _cmd_rule(name: str | None) -> None:
    """Rule command: print a rule's documentation, or the rule table."""
    from sqlty._lint import RULES, rule_doc

    if name is None:
        width = max(map(len, RULES))
        for rule in RULES.values():
            print(f"{rule.name:<{width}}  {rule.default_severity.value:<5}  {rule.description}")
        return
    if name not in RULES:
        logger.error("Unknown rule %r; run `sqlty rule` for the list", name)
        sys.exit(EXIT_ERROR)
    print(rule_doc(name), end="")


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
            files.extend(_walk(path, source))
        else:
            raise ConfigError(f"path does not exist: {path}")
    return files


def _config_file(args: argparse.Namespace) -> Path | None:
    """``--config-file``, else ``SQLTY_CONFIG_FILE`` unless ``--isolated`` (like ty's ``TY_CONFIG_FILE``)."""
    if getattr(args, "config_file", None) is not None:
        return Path(args.config_file)
    env = os.environ.get("SQLTY_CONFIG_FILE")
    return Path(env) if env and not getattr(args, "isolated", False) else None


class ConfigSource:
    """Each directory's ``[tool.sqlty]`` after ``--config-file``, ``--isolated``,
    ``-c/--config``, ``--error/--warn/--ignore`` and the exclude flags.

    By default it is the nearest ``pyproject.toml`` with a table. ``config_file``
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
        from sqlty._config import load_config_file, parse_overrides, validate_rule_severity
        from sqlty._gitignore import GitIgnore

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
        self._exclude = None if exclude is None else self._relative_globs(exclude)
        self._extend_exclude = self._relative_globs(extend_exclude)
        self.force_exclude = force_exclude
        self._respect_gitignore = inline.pop("respect_gitignore", respect_gitignore)
        self.gitignore = GitIgnore()

    def _relative_globs(self, patterns: list[str] | tuple[str, ...]) -> tuple[str, ...]:
        """Command-line globs relative to the current directory: ``./gen`` and ``$PWD/gen`` are ``gen``."""
        globs: list[str] = []
        for pattern in patterns:
            path = Path(pattern)
            if path.is_absolute() and path.is_relative_to(self._cwd):
                pattern = path.relative_to(self._cwd).as_posix() or "."
            while pattern.startswith("./"):
                pattern = pattern[2:]
            globs.append(pattern)
        return tuple(globs)

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
        from sqlty._config import load_config

        if self._isolated:
            config: SQLTyConfig = {}
            config_dir, label = None, "--isolated"
        elif self._fixed is not None:
            config, config_dir, label = self._fixed, self._cwd, str(self._label)
        else:
            config, config_dir = load_config(directory)
            label = f"{config_dir}/pyproject.toml"
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
        from sqlty._config import DEFAULT_EXCLUDE, load_config

        if self._fixed is not None:
            config, base = self._fixed, self._cwd
        else:
            config, config_dir = ({}, None) if self._isolated else load_config(directory)
            # No config: the default globs are names, so the base only matters to ``is_excluded``.
            resolved = directory.resolve()
            base = config_dir or (self._cwd if resolved.is_relative_to(self._cwd) else resolved)
        if self._exclude is None:
            rules = [
                (base, (*config.get("exclude", DEFAULT_EXCLUDE), *config.get("extend_exclude", ())))
            ]
        else:
            rules = [(self._cwd, self._exclude), (base, tuple(config.get("extend_exclude", ())))]
        rules.append((self._cwd, self._extend_exclude))
        return [(b, patterns) for b, patterns in rules if patterns]

    def respects_gitignore(self, directory: Path) -> bool:
        """Whether walking ``directory`` skips what ``.gitignore`` ignores."""
        from sqlty._config import load_config

        if self._respect_gitignore is not None:
            return bool(self._respect_gitignore)
        if self._fixed is not None:
            config = self._fixed
        else:
            config = {} if self._isolated else load_config(directory)[0]
        return config.get("respect_gitignore", True)

    def is_excluded(self, path: Path) -> bool:
        """Whether ``path`` or one of its parents up to a glob's base directory is excluded."""
        resolved = path.resolve()
        for base, patterns in self.exclusions(resolved.parent):
            if not resolved.is_relative_to(base):
                if _is_excluded_entry(resolved.name, None, patterns):
                    return True
                continue
            parts = PurePosixPath(resolved.relative_to(base).as_posix()).parts
            if any(
                _is_excluded_entry(part, PurePosixPath(*parts[:i]), patterns)
                for i, part in enumerate(parts)
            ):
                return True
        return False


@functools.cache
def _exclude_matchers(patterns: tuple[str, ...]) -> tuple[re.Pattern[str], re.Pattern[str]]:
    """Compiled ``(name, relative path)`` matchers for exclude globs; built once per pattern set.

    A glob without a slash matches a file or directory name at any depth;
    every glob also matches the path relative to the config directory.
    """
    names: list[str] = []
    paths: list[str] = []
    for pattern in patterns:
        pattern = pattern.strip("/")
        if "/" not in pattern:
            names.append(fnmatch.translate(pattern))
        paths.append(glob.translate(pattern, recursive=True, include_hidden=True))
    never = r"(?!)"
    return (
        re.compile("|".join(names) or never, re.DOTALL),
        re.compile("|".join(paths) or never, re.DOTALL),
    )


def _is_excluded_entry(
    name: str, relative_dir: PurePosixPath | None, patterns: tuple[str, ...]
) -> bool:
    """Whether the entry ``name`` of ``relative_dir`` matches an exclude glob.

    ``relative_dir`` is the resolved parent relative to the glob's base directory,
    or ``None`` when it lies outside it (only globs without a slash match then).
    """
    by_name, by_path = _exclude_matchers(patterns)
    if by_name.match(name):
        return True
    return relative_dir is not None and bool(by_path.match((relative_dir / name).as_posix()))


def _is_excluded(path: Path, base: Path, patterns: tuple[str, ...]) -> bool:
    """Whether ``path`` matches an exclude glob, relative to ``base`` (``_walk`` batches this per directory)."""
    resolved = path.resolve()
    if not resolved.is_relative_to(base):
        return _is_excluded_entry(resolved.name, None, patterns)
    relative = PurePosixPath(resolved.relative_to(base).as_posix())
    return _is_excluded_entry(relative.name, relative.parent, patterns)


def _is_included(path: Path, base: Path, patterns: tuple[str, ...]) -> bool:
    """Whether ``path`` or a directory holding it matches a glob relative to ``base``, as excludes do."""
    resolved = path.resolve()
    if not resolved.is_relative_to(base):
        return False
    relative = PurePosixPath(resolved.relative_to(base).as_posix())
    return any(
        _is_excluded_entry(entry.name, entry.parent, patterns)
        for entry in [relative, *relative.parents[:-1]]
    )


def _excluded(name: str, rules: list[tuple[PurePosixPath | None, tuple[str, ...]]]) -> bool:
    return any(_is_excluded_entry(name, relative, patterns) for relative, patterns in rules)


def _walk(root: Path, source: ConfigSource) -> list[Path]:
    """The ``.py`` files under ``root``, sorted, without excluded directories and files."""
    found: list[Path] = []
    for directory, dirnames, filenames in os.walk(root):
        current = Path(directory)
        resolved = current.resolve()
        rules = [
            (
                PurePosixPath(resolved.relative_to(base).as_posix())
                if resolved.is_relative_to(base)
                else None,
                patterns,
            )
            for base, patterns in source.exclusions(current)
        ]
        ignore = source.gitignore if source.respects_gitignore(current) else None
        dirnames[:] = sorted(
            d
            for d in dirnames
            if not _excluded(d, rules) and not (ignore and ignore.ignored(resolved, d, is_dir=True))
        )
        found.extend(
            current / name
            for name in sorted(filenames)
            if name.endswith(".py")
            and not _excluded(name, rules)
            and not (ignore and ignore.ignored(resolved, name, is_dir=False))
        )
    return sorted(found)


@functools.cache
def _is_package(directory: Path) -> bool:
    return (directory / "__init__.py").exists()


@functools.cache
def _package_path(directory: Path) -> tuple[str, ...]:
    """Names of ``directory`` and its parents up through the ``__init__.py`` ones."""
    if directory == directory.parent or not _is_package(directory):
        return ()
    return (*_package_path(directory.parent), directory.name)


@functools.cache
def _resolved_root(root: Path) -> Path | None:
    """``root`` resolved, or ``None`` when it is not a directory."""
    root = root.resolve()
    return root if root.is_dir() else None


def _checked_roots(paths: Iterable[Path], files: Iterable[Path]) -> list[Path]:
    """The checked directories, the parents of checked files and the package roots of ``files``.

    Example:
        >>> [root.name for root in _checked_roots([Path("/missing/q.py")], [])]
        ['missing']
    """
    roots = [path if path.is_dir() else path.parent for path in paths]
    for file in files:
        directory = file.parent
        for _ in _package_path(directory):
            directory = directory.parent
        roots.append(directory)
    return list(dict.fromkeys(root.resolve() for root in roots))


def _module_names(path: Path, roots: list[Path]) -> list[str]:
    """Dotted names the resolved ``path`` can be imported by, the likeliest first.

    The package path (up through the ``__init__.py`` directories) comes first,
    then the path from each checked directory, with and without its own name.
    ``pkg/__init__.py`` is ``pkg``.

    Example:
        >>> _module_names(Path("missing/pkg/mod.py"), [])
        ['mod']
    """
    parts = [*_package_path(path.parent), *([] if path.stem == "__init__" else [path.stem])]
    names = [".".join(parts)]
    for root in map(_resolved_root, roots):
        if root is not None and path.is_relative_to(root):
            relative = list(path.relative_to(root).with_suffix("").parts)
            if relative[-1] == "__init__":
                relative.pop()
            names += [".".join(relative), ".".join([root.name, *relative])]
    return list(dict.fromkeys(name for name in names if name)) or [path.stem]


@dataclass(frozen=True)
class Settings:
    """``[tool.sqlty]`` of the nearest ``pyproject.toml``, with CLI flags on top."""

    dialect: str = "postgres"
    schema: Schema | None = None
    schema_sql: str | None = None
    types: dict[str, str] | None = None
    import_style: str | None = None
    untyped: bool = False
    json_types: str = "typing.Any"
    rules: dict[str, str] | None = None
    type_modules: tuple[str, ...] = ()
    query_variants: int | None = None
    functions: tuple[tuple[str, str, str | None], ...] = ()
    format_command: tuple[str, ...] = ()
    overrides: tuple[RuleOverride, ...] = ()
    config_dir: Path | None = None


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
        from sqlty.core import load_schema, schema_skipped_objects

        key = (schema_sql, dialect, tuple(sorted((types or {}).items())))
        if (schema := self._schemas.get(key)) is None:
            try:
                schema = self._schemas[key] = load_schema(schema_sql, dialect=dialect, types=types)
            except ValueError as exc:
                raise ConfigError(f"[tool.sqlty.types]: {exc}") from None
            for kind, name in schema_skipped_objects(schema, dialect=dialect):
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
        directory = self._directory(path.parent)
        if directory not in self._by_dir:
            self._by_dir[directory] = self._resolve(directory)
        settings = self._by_dir[directory]
        if not settings.overrides or settings.config_dir is None:
            return settings
        rules = dict(settings.rules or {})
        for override in settings.overrides:
            include, exclude = tuple(override["include"]), tuple(override["exclude"])
            if (not include or _is_included(path, settings.config_dir, include)) and not (
                exclude and _is_included(path, settings.config_dir, exclude)
            ):
                rules |= override["rules"]
        return replace(settings, rules=rules | self.source.cli_rules)

    def _resolve(self, directory: Path) -> Settings:
        from sqlty._config import ConfigError
        from sqlty.core import strip_source

        config, config_dir, label = self.source.load(directory)
        args = self._args
        schema_sql = self._schema_sql
        if schema_sql is None and "schema" in config and config_dir is not None:
            schema_sql = _load_schema(config["schema"], config_dir)
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
        from sqlty._config import MAX_QUERY_VARIANTS

        dialect = args.dialect or config.get("dialect", "postgres")
        return Settings(
            dialect=dialect,
            schema=None
            if schema_sql is None
            else self._schema(schema_sql, dialect, config.get("types")),
            schema_sql=schema_sql,
            types=config.get("types"),
            import_style=import_style,
            untyped=untyped == "sentinel",
            json_types=json_types,
            rules=config.get("rules"),
            type_modules=tuple(config.get("type_modules", ())),
            query_variants=config.get("max_query_variants", MAX_QUERY_VARIANTS)
            if config.get("preview")
            else None,
            functions=tuple(
                (f["import_path"], f["query_param"], f.get("return_type_param"))
                for f in config.get("functions", [])
            ),
            format_command=tuple(config.get("format_command", ())),
            overrides=tuple(config.get("overrides", ())),
            config_dir=config_dir,
        )


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


def _print_diff(path: Path, original: str, modified: str) -> None:
    """Print unified diff between original and modified."""
    import difflib

    diff = difflib.unified_diff(
        original.splitlines(keepends=True),
        modified.splitlines(keepends=True),
        fromfile=str(path),
        tofile=str(path),
    )
    sys.stdout.writelines(diff)
