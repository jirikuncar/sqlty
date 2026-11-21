"""Tests go through each package's public API: the counts of the rest only go down."""

import re
from pathlib import Path

ROOT = Path(__file__).parent.parent
LIMITS = ROOT / "scripts" / "internal-tests.txt"
PRIVATE = re.compile(
    r"^\s*(?:from|import)\s+sqlty(?:\.\w+)*\._(?!core\b)\w+"
    r"|^\s*from\s+sqlty(?:\.\w+)*\s+import\s+[^#\n]*\b_(?!core\b)\w+",
    re.MULTILINE,
)


def counts() -> dict[str, int]:
    found = {
        crate.name: sum(path.read_text().count("#[test]") for path in (crate / "src").rglob("*.rs"))
        for crate in (ROOT / "rust" / "crates").iterdir()
        if (crate / "src").is_dir()
    }
    found["python"] = sum(
        bool(PRIVATE.search(path.read_text())) for path in (ROOT / "tests").rglob("*.py")
    )
    return found


def limits() -> dict[str, int]:
    lines = LIMITS.read_text().splitlines()
    return {name: int(n) for name, n in (line.split() for line in lines if line and line[0] != "#")}


def test_no_new_tests_past_the_public_api() -> None:
    allowed = limits()
    over = {name: n for name, n in counts().items() if n > allowed.get(name, 0)}
    assert not over, f"tests past the public API, over {LIMITS.name}: {over}; move them to tests/"


def test_limits_are_current() -> None:
    assert limits() == counts(), f"lower {LIMITS.name} to the current counts"
