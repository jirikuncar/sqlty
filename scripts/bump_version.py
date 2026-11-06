"""Set the release version everywhere it is written, or check that every place agrees.

    uv run python scripts/bump_version.py 0.4.0            # write 0.4.0 to every place
    uv run python scripts/bump_version.py 0.4.0 --dry-run  # list what would change
    uv run python scripts/bump_version.py --check          # exit 1 if a place disagrees

``rust/Cargo.toml`` (``[workspace.package]``) is the source of truth: ``pyproject.toml``
reads it, and ``--check`` compares every other place with it. ``PLACES`` lists them, each
with how often the version appears there. The VS Code extension takes its version from
the release tag in CI. ``tests/test_docs.py`` runs the ``--check`` comparison.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VERSION = r"(?P<version>\d+\.\d+\.\d+[\w.+-]*)"
JSON_VERSION = re.compile(rf'"version": "{VERSION}"')
PRE_COMMIT_REV = re.compile(rf"rev: v{VERSION}\n")
PLACES: list[tuple[str, re.Pattern[str], int]] = [
    ("rust/Cargo.toml", re.compile(rf'\[workspace\.package\]\nversion = "{VERSION}"'), 1),
    # Workspace members are the packages without a `source`.
    ("rust/Cargo.lock", re.compile(rf'name = "sqlty[\w-]*"\nversion = "{VERSION}"\n(?!source)'), 7),
    (".claude-plugin/plugin.json", JSON_VERSION, 1),
    (".claude-plugin/marketplace.json", JSON_VERSION, 2),
    (".codex-plugin/plugin.json", JSON_VERSION, 1),
    (".cursor-plugin/plugin.json", JSON_VERSION, 1),
    ("docs/recipes/ci.md", PRE_COMMIT_REV, 1),
    ("skills/sqlty-ci/SKILL.md", PRE_COMMIT_REV, 1),
]


def current() -> str:
    """The version in ``rust/Cargo.toml``."""
    path, pattern, _ = PLACES[0]
    found = pattern.search((ROOT / path).read_text())
    assert found is not None, f"{path}: no [workspace.package] version"
    return found["version"]


def bump(version: str, *, write: bool = True) -> list[str]:
    """Set ``version`` in every place; the places that changed or would change."""
    changed = []
    for path, pattern, count in PLACES:
        text = (ROOT / path).read_text()
        found = list(pattern.finditer(text))
        if len(found) != count:
            raise ValueError(f"{path}: expected the version {count} time(s), found {len(found)}")
        new = text
        for match in reversed(found):
            start, end = match.span("version")
            new = new[:start] + version + new[end:]
        if new != text:
            changed.append(path)
            if write:
                (ROOT / path).write_text(new)
    return changed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("version", nargs="?", help="the new version, without a leading v")
    parser.add_argument(
        "--check", action="store_true", help="fail if a place disagrees with rust/Cargo.toml"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="list what would change, write nothing"
    )
    args = parser.parse_args()
    if args.check == (args.version is not None):
        parser.error("pass a version or --check")
    version = current() if args.check else args.version.removeprefix("v")
    changed = bump(version, write=not (args.check or args.dry_run))
    for path in changed:
        print(
            f"{path}: {'disagrees with rust/Cargo.toml' if args.check else version}",
            file=sys.stderr,
        )
    return 1 if changed and args.check else 0


if __name__ == "__main__":
    sys.exit(main())
