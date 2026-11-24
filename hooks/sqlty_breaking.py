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
    sqlty = installed(root)
    if sqlty is None:
        return
    against = baseline(root)
    if against is None:
        return
    try:
        run = subprocess.run(
            [sqlty, "breaking", "--against", against],
            cwd=root,
            capture_output=True,
            text=True,
            timeout=TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return
    findings = run.stdout.strip()
    if run.returncode not in (0, 1) or not findings:
        return
    lines = findings.splitlines()
    findings = "\n".join(sorted(lines, key=lambda line: edited.name not in line))
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
    """The project's own sqlty, as `uv add --dev sqlty` installs it, else the one on `PATH`."""
    for venv in [root, *root.parents]:
        for script in (venv / ".venv" / "bin" / "sqlty", venv / ".venv" / "Scripts" / "sqlty.exe"):
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
