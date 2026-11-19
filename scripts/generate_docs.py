"""Render the reference tables of ``docs/reference/{configuration,cli}.md``.

    uv run python scripts/generate_docs.py            # rewrite the generated blocks
    uv run python scripts/generate_docs.py --check    # exit 1 if a page is out of date

Only the text between ``<!-- generated:NAME -->`` and ``<!-- /generated:NAME -->``
is owned by this script; everything else in the pages is hand-written. The
sources are ``KEY_DOCS`` / ``FUNCTION_KEY_DOCS`` / ``DEFAULT_EXCLUDE`` in
``sqlty._config`` and the clap commands of ``sqlty-cli`` (``sqlty._core.cli_reference``).
``tests/test_docs.py`` runs the ``--check`` comparison.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REFERENCE = ROOT / "docs" / "reference"
COMMANDS = ("rewrite", "strip", "check", "complete", "breaking", "rule", "skill")
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


def _usage(usage: str) -> str:
    return f"```text\n{usage}\n```\n"


def _help(text: str) -> str:
    return text.replace("\n\n", ". ").replace("\n", " ")


def cli_blocks() -> dict[str, str]:
    from sqlty._core import cli_reference

    commands = {
        name: (usage, {o: h for o, h in options if o != "-h, --help"})
        for name, usage, options in cli_reference()
    }
    shared = set.intersection(*(set(commands[c][1]) for c in COMMANDS[:3]))
    blocks = {"cli-usage": _usage(commands["sqlty"][0])}
    rewrite = commands["rewrite"][1]
    blocks["cli-options-common"] = _table(
        ("Option", "Description"), [(f"`{o}`", _help(rewrite[o])) for o in rewrite if o in shared]
    )
    for name in COMMANDS:
        usage, options = commands[name]
        blocks[f"cli-usage-{name}"] = _usage(usage)
        rows = [(f"`{o}`", _help(h)) for o, h in options.items() if o not in shared and h]
        if name != "rule":
            blocks[f"cli-options-{name}"] = _table(("Option", "Description"), rows)
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
