"""Render the reference tables of ``docs/reference/{configuration,cli}.md``.

    uv run python scripts/generate_docs.py            # rewrite the generated blocks
    uv run python scripts/generate_docs.py --check    # exit 1 if a page is out of date

Only the text between ``<!-- generated:NAME -->`` and ``<!-- /generated:NAME -->``
is owned by this script; everything else in the pages is hand-written. The
sources are ``KEY_DOCS`` / ``FUNCTION_KEY_DOCS`` / ``DEFAULT_EXCLUDE`` in
``sqlty._config`` and the argparse parser from ``sqlty._cli.build_parser``.
``tests/test_docs.py`` runs the ``--check`` comparison.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REFERENCE = ROOT / "docs" / "reference"
COMMANDS = ("rewrite", "strip", "check", "rule")
BLOCK = re.compile(
    r"(?P<open><!-- generated:(?P<name>[\w-]+) -->\n)(?P<body>.*?)(?P<close><!-- /generated:(?P=name) -->)",
    re.S,
)


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
        FUNCTION_KEY_DOCS,
        KEY_DOCS,
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


def _subparsers(parser: argparse.ArgumentParser) -> dict[str, argparse.ArgumentParser]:
    for action in parser._actions:
        if isinstance(action, argparse._SubParsersAction):
            return dict(action.choices)
    raise AssertionError("no subcommands")


def _usage(parser: argparse.ArgumentParser) -> str:
    # Continuation lines are indented to the end of "usage: "; drop that prefix.
    lines = parser.format_usage().rstrip("\n").split("\n")
    lines = [lines[0].removeprefix("usage: "), *(line[len("usage: ") :] for line in lines[1:])]
    return "```\n" + "\n".join(lines) + "\n```\n"


def _invocation(parser: argparse.ArgumentParser, action: argparse.Action) -> str:
    text = parser._get_formatter()._format_action_invocation(action).replace(", ", "`, `")
    return f"`{text} ...`" if action.nargs == "*" and not action.option_strings else f"`{text}`"


def _options(parser: argparse.ArgumentParser, skip: set[str]) -> str:
    rows = [
        (_invocation(parser, a), a.help or "")
        for a in parser._actions
        if a.dest not in skip and a.help
    ]
    return _table(("Option", "Description"), rows)


def cli_blocks() -> dict[str, str]:
    from sqlty._cli import build_parser

    # Wrapped usage depends on the terminal width.
    os.environ["COLUMNS"] = "80"
    parser = build_parser()
    commands = _subparsers(parser)
    blocks = {"cli-usage": _usage(parser)}
    shared = set.intersection(*({a.dest for a in commands[c]._actions} for c in COMMANDS[:3]))
    blocks["cli-options-common"] = _table(
        ("Option", "Description"),
        [
            (_invocation(commands["rewrite"], a), a.help or "")
            for a in commands["rewrite"]._actions
            if a.dest in shared
        ],
    )
    for name in COMMANDS:
        blocks[f"cli-usage-{name}"] = _usage(commands[name])
        if name != "rule":
            blocks[f"cli-options-{name}"] = _options(commands[name], skip=shared)
    return blocks


PAGES = {"configuration.md": config_blocks, "cli.md": cli_blocks}


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
