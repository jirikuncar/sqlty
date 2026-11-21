"""Render the reference tables of ``docs/reference/{configuration,cli,rules}.md``.

    uv run python scripts/generate_docs.py            # rewrite the generated blocks
    uv run python scripts/generate_docs.py --check    # exit 1 if a page is out of date

Only the text between ``<!-- generated:NAME -->`` and ``<!-- /generated:NAME -->``
is owned by this script; everything else in the pages is hand-written. The
sources are ``KEY_DOCS`` / ``FUNCTION_KEY_DOCS`` below, ``DEFAULT_EXCLUDE`` in
``sqlty._config``, the clap commands of ``sqlty-cli`` (``sqlty._core.cli_reference``) and
the rule pages' front matter (``sqlty._lint.RULES``).
``tests/test_docs.py`` runs the ``--check`` comparison.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections.abc import Iterable
from pathlib import Path
from typing import NamedTuple

ROOT = Path(__file__).resolve().parent.parent
REFERENCE = ROOT / "docs" / "reference"
COMMANDS = (
    "rewrite",
    "strip",
    "format",
    "check",
    "complete",
    "breaking",
    "rule",
    "skill",
    "server",
    "version",
)
BLOCK = re.compile(
    r"(?P<open><!-- generated:(?P<name>[\w-]+) -->\n)(?P<body>.*?)(?P<close><!-- /generated:(?P=name) -->)",
    re.S,
)


class KeyDoc(NamedTuple):
    """Reference documentation of one configuration key.

    ``docs/reference/configuration.md`` renders its tables from these entries
    (``make generate-docs``); ``tests/test_docs.py`` fails when the page drifts.
    """

    type: str
    default: str
    used_by: str
    summary: str
    required: bool = False


KEY_DOCS: dict[str, KeyDoc] = {
    "dialect": KeyDoc(
        '`"postgres"`, `"sqlite"`, `"mysql"`, ...',
        '`"postgres"`',
        "`rewrite`, `check`",
        "SQL dialect of queries and schema",
    ),
    "driver": KeyDoc(
        '`"psycopg"`',
        "none",
        "`rewrite`, `check`",
        "Driver whose own classes type the columns the default mapping leaves untyped: PostgreSQL ranges",
    ),
    "schema": KeyDoc(
        "string or list of strings",
        "none",
        "`rewrite`, `check`",
        "Schema `.sql` files, directories or globs, relative to the configuration file",
    ),
    "queries": KeyDoc(
        "string or list of strings",
        "none",
        "`check`",
        "`.sql` files of named queries (aiosql, sqlc, PugSQL), directories or globs, relative to the configuration file",
    ),
    "query-format": KeyDoc(
        '`"auto"`, `"aiosql"`, `"sqlc"`, `"pugsql"`, `"file"`',
        '`"auto"`',
        "`check`",
        "How `queries` files name their queries; `auto` reads each file's first header",
    ),
    "rules": KeyDoc(
        "table of rule = severity",
        "per-rule defaults",
        "`check`",
        'Severity per lint rule: `"error"`, `"warn"` or `"ignore"`',
    ),
    "overrides": KeyDoc(
        "array of tables",
        "`[]`",
        "`check`",
        "Rule severities for the files `include` matches and `exclude` does not, on top of `rules`",
    ),
    "import-style": KeyDoc(
        "string",
        '`"from sqlty import SQL"`',
        "`rewrite`, `strip`, `check`, magic codec",
        "How rewritten code imports and spells the wrapper",
    ),
    "untyped": KeyDoc(
        '`"skip"`, `"sentinel"`',
        '`"skip"`',
        "`rewrite`",
        "Whether queries of unknown row type are wrapped as `SQL[UNTYPED]`",
    ),
    "json-types": KeyDoc(
        '`"typing.Any"`, `"typing.TypedDict"`',
        '`"typing.Any"`',
        "`rewrite`, `check --fix`",
        "How a JSON column of known shape is typed: `typing.Any`, or a generated `TypedDict`",
    ),
    "param-types": KeyDoc(
        "`true`, `false`",
        "`true`",
        "`rewrite`, `check --fix`",
        "Whether `SQL[R]` calls get the type of their parameters, `SQL[R, P]`",
    ),
    "preview": KeyDoc(
        "boolean",
        "`false`",
        "`check`",
        "Turn on features still in preview: query variants",
    ),
    "max-query-variants": KeyDoc(
        "positive integer",
        "`64`",
        "`check`",
        "With `preview`, the most variants of a query built in branches that `check` checks",
    ),
    "coerce-row-types": KeyDoc(
        "list of strings",
        "`[]`",
        "`check`",
        "`TypedDict`, `NamedTuple` and dataclass rows, as `module.Name`, the code validates with Pydantic",
    ),
    "functions": KeyDoc(
        "array of tables",
        "`[]`",
        "`check`",
        "Helper functions whose string argument is a query",
    ),
    "types": KeyDoc(
        "table of SQL type or `table.column` = Python type",
        "`{}`",
        "`rewrite`, `check`",
        "Python types a converter above the driver gives a SQL type or a column",
    ),
    "format-command": KeyDoc(
        "list of strings",
        "none",
        "`rewrite`, `check --fix`",
        "Formatter run on each file `rewrite` or `check --fix` changes, with the path appended",
    ),
    "extend": KeyDoc(
        "string",
        "none",
        "`rewrite`, `strip`, `check`, magic codec",
        "Another configuration file this one is applied on top of, relative to this file",
    ),
    "databases": KeyDoc(
        "table of tables",
        "`{}`",
        "`rewrite`, `check`",
        "Named databases, each with its own `dialect`, `schema` and the files it applies to",
    ),
    "exclude": KeyDoc(
        "list of globs",
        "see [Default excludes](#exclude-and-extend-exclude)",
        "`rewrite`, `strip`, `check`",
        "Globs skipped while walking a directory; replaces the default list",
    ),
    "extend-exclude": KeyDoc(
        "list of globs",
        "`[]`",
        "`rewrite`, `strip`, `check`",
        "Globs skipped in addition to `exclude` or the defaults",
    ),
    "respect-gitignore": KeyDoc(
        "boolean",
        "`true`",
        "`rewrite`, `strip`, `check`",
        "Skip the files `.gitignore` ignores while walking a directory",
    ),
}
"""Reference docs per key of ``[tool.sqlty]``, in documentation order."""

FUNCTION_KEY_DOCS: dict[str, KeyDoc] = {
    "import-path": KeyDoc(
        "string",
        "none",
        "`check`",
        'Module path and function name, for example `"app.db.sql"`; a method is `"pkg.db.Db.fetch_all"`. '
        "A package that re-exports it, `pkg.Db.fetch_all`, works too. "
        'Use `"__main__.sql"` or the file name for a script run directly',
        required=True,
    ),
    "query-param": KeyDoc(
        "string",
        "none",
        "`check`",
        "Name of the parameter that holds the query",
        required=True,
    ),
    "return-type-param": KeyDoc(
        "string",
        "none",
        "`check`",
        "Parameter whose argument, such as `return_tp=int`, is the row type of that call "
        "instead of an inferred one; `check` compares it with the query columns",
    ),
}
"""Reference docs per key of a ``[[tool.sqlty.functions]]`` entry."""


def _cell(text: str) -> str:
    return text.replace("|", "\\|")


def _table(header: tuple[str, ...], rows: list[tuple[str, ...]]) -> str:
    lines = [
        "| " + " | ".join(header) + " |",
        "|" + "|".join("-" * (len(h) + 2) for h in header) + "|",
        *("| " + " | ".join(_cell(c) for c in row) + " |" for row in rows),
    ]
    return "\n".join(lines) + "\n"


def _anchor(key: str) -> str:
    return "exclude-and-extend-exclude" if key in ("exclude", "extend-exclude") else key


def config_blocks() -> dict[str, str]:
    from sqlty._config import (
        DEFAULT_EXCLUDE,
        DISCOVERY_KEYS,
        TOOL_KEYS,
    )

    assert set(KEY_DOCS) == {*TOOL_KEYS, *DISCOVERY_KEYS}, "KEY_DOCS is out of sync with the keys"
    keys = _table(
        ("Key", "Type", "Default", "Used by", "Summary"),
        [
            (f"[`{key}`](#{_anchor(key)})", d.type, d.default, d.used_by, d.summary)
            for key, d in KEY_DOCS.items()
        ],
    )
    functions = _table(
        ("Key", "Required", "Description"),
        [
            (f"`{key}`", "yes" if d.required else "no", d.summary)
            for key, d in FUNCTION_KEY_DOCS.items()
        ],
    )
    excludes = ", ".join(f"`{name}`" for name in DEFAULT_EXCLUDE)
    return {
        "config-keys": keys,
        "config-function-keys": functions,
        "config-default-excludes": f"{excludes}\n",
    }


def _usage(usage: str) -> str:
    return f"```text\n{usage}\n```\n"


def _help(text: str) -> str:
    return text.replace("\n\n", ". ").replace("\n", " ")


def cli_blocks() -> dict[str, str]:
    """Every option of every command: the ones ``rewrite``, ``strip`` and ``check`` share
    in one table, listed once for each command that takes them all."""
    from sqlty._core import cli_reference

    commands = {name: (usage, dict(options)) for name, usage, options in cli_reference()}
    shared = set.intersection(*(set(commands[c][1]) for c in ("rewrite", "strip", "check")))
    usage, root = commands["sqlty"]
    rewrite = commands["rewrite"][1]
    blocks = {
        "cli-usage": _usage(usage),
        "cli-options": _options(root.items()),
        "cli-options-common": _options((o, h) for o, h in rewrite.items() if o in shared),
        "cli-common-commands": _and(
            [f"`{name}`" for name in COMMANDS if shared <= set(commands[name][1])]
        )
        + " accept:\n",
    }
    for name in COMMANDS:
        usage, options = commands[name]
        own = (
            options.items()
            if not shared <= set(options)
            else ((o, h) for o, h in options.items() if o not in shared)
        )
        blocks[f"cli-usage-{name}"] = _usage(usage)
        blocks[f"cli-options-{name}"] = _options(own)
    return blocks


def _and(items: list[str]) -> str:
    return ", ".join(items[:-1]) + " and " + items[-1]


def _options(options: Iterable[tuple[str, str]]) -> str:
    return _table(("Option", "Description"), [(f"`{o}`", _help(h)) for o, h in options])


def rules_blocks() -> dict[str, str]:
    from sqlty._lint import RULES

    return {
        "rules": _table(
            ("Rule", "Group", "Default", "Fixable", "Reports"),
            [
                (
                    f"[`{r.name}`](rules/{r.name}.md)",
                    r.group,
                    r.default_severity.value,
                    "yes" if r.fixable else "no",
                    r.description,
                )
                for r in RULES.values()
            ],
        )
    }


PAGES = {"configuration.md": config_blocks, "cli.md": cli_blocks, "rules.md": rules_blocks}


def render(text: str, blocks: dict[str, str], page: str) -> str:
    seen: set[str] = set()

    def replace(match: re.Match[str]) -> str:
        name = match["name"]
        if name not in blocks:
            raise SystemExit(f"{page}: unknown generated block {name!r}")
        seen.add(name)
        return match["open"] + blocks[name] + match["close"]

    out = BLOCK.sub(replace, text)
    if missing := set(blocks) - seen:
        raise SystemExit(f"{page}: no marker for generated block(s): {', '.join(sorted(missing))}")
    return out


def generate(check: bool = False) -> list[Path]:
    """Rewrite the pages (or only report the stale ones when ``check``)."""
    stale: list[Path] = []
    for page, blocks in PAGES.items():
        path = REFERENCE / page
        old = path.read_text()
        new = render(old, blocks(), page)
        if new != old:
            stale.append(path)
            if not check:
                path.write_text(new)
    return stale


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="fail if a page is out of date")
    check = parser.parse_args().check
    stale = generate(check)
    for path in stale:
        print(f"{path.relative_to(ROOT)}: {'out of date' if check else 'updated'}", file=sys.stderr)
    return 1 if stale and check else 0


if __name__ == "__main__":
    sys.exit(main())
