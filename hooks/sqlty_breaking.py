"""Claude Code `PostToolUse` hook: after an edit to a `.sql` or migration file of a sqlty
project, run `sqlty breaking` against the branch point and hand its findings to the agent.

Silent when sqlty, git or a `[tool.sqlty]` configuration is missing, so it never blocks an
edit. Reads only the hook event on stdin; runs only `git` and `sqlty`.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

LIMIT = 4000
TIMEOUT = 100


def main() -> None:
    event = json.load(sys.stdin)
    edited = Path(event.get("cwd") or ".") / (
        (event.get("tool_input") or {}).get("file_path") or ""
    )
    if edited.suffix != ".sql" and "migrations" not in edited.parts:
        return
    root = project_root(edited.parent)
    if root is None:
        return
    relative = edited.relative_to(root)
    if edited.suffix != ".sql" and "migrations" not in relative.parts:
        return
    sqlty = installed(root)
    if sqlty is None:
        return
    against = baseline(root)
    if against is None:
        return
    try:
        run = subprocess.run(
            [sqlty, "breaking", "--against", against, "--output-format", "json"],
            cwd=root,
            capture_output=True,
            text=True,
            timeout=TIMEOUT,
        )
    except subprocess.TimeoutExpired, OSError:
        return
    if run.returncode not in (0, 1):
        return
    try:
        found = json.loads(run.stdout)
    except json.JSONDecodeError:
        return
    if not found:
        return
    found.sort(key=lambda finding: finding["file"] != relative.as_posix())
    findings = "\n".join(
        f"{f['file']}:{f['line']}: [{f['rule']}] {f['message']} ({f['direction']})" for f in found
    )
    if len(findings) > LIMIT:
        findings = findings[:LIMIT] + "\n..."
    context = (
        f"`sqlty breaking --against {against}` on this branch, the edited file first:\n{findings}\n"
        "Fix them, or see the sqlty-breaking skill."
    )
    json.dump(
        {"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": context}},
        sys.stdout,
    )


def project_root(directory: Path) -> Path | None:
    """The nearest directory whose `sqlty.toml` or `pyproject.toml` configures sqlty."""
    for parent in [directory, *directory.parents]:
        if (parent / "sqlty.toml").is_file():
            return parent
        pyproject = parent / "pyproject.toml"
        if pyproject.is_file() and "[tool.sqlty" in pyproject.read_text(errors="replace"):
            return parent
    return None


def installed(root: Path) -> str | None:
    """The project's own sqlty, as `uv add --dev sqlty` installs it, else the one on `PATH`.

    Only the `.venv` of the project itself: one in a directory above it is not the user's.
    """
    for script in (root / ".venv" / "bin" / "sqlty", root / ".venv" / "Scripts" / "sqlty.exe"):
        if script.is_file():
            return str(script)
    return shutil.which("sqlty")


def baseline(root: Path) -> str | None:
    """Where the branch left the default branch of `origin`, else `HEAD`."""

    def git(*args: str) -> str | None:
        found = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
        return found.stdout.strip() if found.returncode == 0 else None

    if shutil.which("git") is None or git("rev-parse", "--verify", "HEAD") is None:
        return None
    default = git("rev-parse", "--abbrev-ref", "origin/HEAD")
    return (default and git("merge-base", "HEAD", default)) or "HEAD"


if __name__ == "__main__":
    main()
