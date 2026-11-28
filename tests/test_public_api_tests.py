"""Tests go through each package's public API: the counts of the rest only go down."""

import ast
import re
from pathlib import Path

ROOT = Path(__file__).parent.parent
LIMITS = ROOT / "scripts" / "internal-tests.txt"
RUST_TEST = re.compile(r"^\s*#\[(?:\w+::)*test(?:_case)?\b", re.MULTILINE)


def private(name: str) -> bool:
    """A private `sqlty._*` module or name: `sqlty._core` is sqlty-ext's public API."""
    return any(
        part.startswith("_") and part != "_core" and not part.endswith("__")
        for part in name.split(".")
    )


def private_uses(source: str) -> int:
    """Imports, attributes and module strings in `source` that reach a private part of sqlty."""
    modules: set[str] = set()
    attributes: list[ast.Attribute] = []
    uses = 0
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Attribute):
            attributes.append(node)
        elif isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name.split(".")[0] == "sqlty":
                    uses += private(alias.name)
                    modules.add(alias.asname or "sqlty")
        elif isinstance(node, ast.ImportFrom) and (node.module or "").split(".")[0] == "sqlty":
            uses += sum(private(f"{node.module}.{alias.name}") for alias in node.names)
            modules.update(alias.asname or alias.name for alias in node.names)
        elif isinstance(node, ast.Constant) and isinstance(node.value, str):
            uses += bool(re.fullmatch(r"sqlty(\.\w+)+", node.value) and private(node.value))
    return uses + sum(
        ast.unparse(a.value).split(".")[0] in modules and private(a.attr) for a in attributes
    )


def counts() -> dict[str, int]:
    found = {
        crate.name: sum(
            len(RUST_TEST.findall(path.read_text())) for path in (crate / "src").rglob("*.rs")
        )
        for crate in (ROOT / "rust" / "crates").iterdir()
        if (crate / "src").is_dir()
    }
    found["python"] = sum(
        private_uses(path.read_text()) for path in (ROOT / "tests").rglob("test_*.py")
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
    current = {name: n for name, n in counts().items() if n}
    assert limits() == current, f"set {LIMITS.name} to the current counts"
