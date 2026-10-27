"""Fail when a branch weakens the no-exec harness (AGENTS.md#no-exec).

    python scripts/check_no_exec_harness.py origin/main    # compare HEAD with its merge base

CI runs the copy of this script from the base branch, so a branch cannot edit the rules it
is held to. Against the merge base, HEAD must:

- keep the guard (``tests/_no_exec.py``, ``tests/test_no_exec.py``, this script) unchanged
  and enabled in ``pyproject.toml``;
- only remove lines from ``tests/no_exec_allowlist.txt``;
- keep every mdtest assertion (``# error:``, ``-- columns:``, ...) of the base; a bare
  ``# error:`` may become a specific one;
- add no skip or xfail, and remove no test function.

Needs only the standard library and git.
"""

from __future__ import annotations

import collections
import re
import subprocess
import sys

GUARD = ("tests/_no_exec.py", "tests/test_no_exec.py", "scripts/check_no_exec_harness.py")
ALLOWLIST = "tests/no_exec_allowlist.txt"
MARKDOWN = re.compile(r"^(tests/mdtest/|docs/|src/sqlty/rules/|skills/|README\.md$).*\.md$")
ASSERTION = re.compile(
    r"^\s*(#\s*(error|rewritten|rewritten-file|stripped|fixed|decoded):"
    r"|--\s*(inferred|expanded|columns|json|rules|params):).*$",
    re.MULTILINE,
)
BARE_ERROR = re.compile(r"^\s*#\s*error:\s*$")
SKIPS = re.compile(
    r"mdtest:skip|mdtest:xfail|```python ignore|pytest\.mark\.(skip|skipif|xfail)"
    r"|pytest\.(skip|xfail|importorskip)\("
)
TEST = re.compile(r"^\s*(async\s+)?def test_\w*", re.MULTILINE)


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def files(rev: str) -> dict[str, str]:
    """The text of the Markdown and Python files that hold tests, at ``rev``."""
    names = [
        n
        for n in git("ls-tree", "-r", "--name-only", rev).splitlines()
        if MARKDOWN.match(n) or (n.startswith("tests/") and n.endswith(".py"))
    ]
    return {n: git("show", f"{rev}:{n}") for n in names}


def show(rev: str, path: str) -> str | None:
    try:
        return git("show", f"{rev}:{path}")
    except subprocess.CalledProcessError:
        return None


def problems(base: str) -> list[str]:
    found: list[str] = []
    for path in GUARD:
        if (old := show(base, path)) is not None and old != show("HEAD", path):
            found.append(f"{path} changed")
    if "tests._no_exec" not in (show("HEAD", "pyproject.toml") or ""):
        found.append("pyproject.toml no longer loads the tests._no_exec plugin")
    if (old := show(base, ALLOWLIST)) is not None:
        added = set((show("HEAD", ALLOWLIST) or "").splitlines()) - set(old.splitlines())
        found += [f"{ALLOWLIST} gained {line!r}" for line in sorted(added)]

    before, after = files(base), files("HEAD")
    asserted = {
        rev: collections.Counter(
            m.group(0).strip()
            for name, text in texts.items()
            if name.endswith(".md")
            for m in ASSERTION.finditer(text)
        )
        for rev, texts in (("base", before), ("head", after))
    }
    specific = {
        rev: collections.Counter({k: v for k, v in c.items() if not BARE_ERROR.match(k)})
        for rev, c in asserted.items()
    }
    for line, count in sorted((specific["base"] - specific["head"]).items()):
        found.append(f"assertion removed {count}x: {line}")
    bare = {rev: sum(c.values()) - sum(specific[rev].values()) for rev, c in asserted.items()}
    gained = sum((specific["head"] - specific["base"]).values())
    if bare["head"] + gained < bare["base"]:
        found.append(f"bare '# error:' assertions dropped: {bare['base']} -> {bare['head']}")

    def count(pattern: re.Pattern[str], texts: dict[str, str]) -> int:
        return sum(len(pattern.findall(text)) for text in texts.values())

    if (n := count(SKIPS, after)) > (m := count(SKIPS, before)):
        found.append(f"skips and xfails grew: {m} -> {n}")
    if (n := count(TEST, after)) < (m := count(TEST, before)):
        found.append(f"test functions dropped: {m} -> {n}")
    return found


def main() -> int:
    target = sys.argv[1] if len(sys.argv) > 1 else "origin/main"
    base = git("merge-base", target, "HEAD").strip()
    if found := problems(base):
        print(
            "The no-exec harness was weakened (see AGENTS.md#no-exec):",
            *found,
            sep="\n  ",
        )
        return 1
    print(f"no-exec harness intact against {target} ({base[:12]})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
