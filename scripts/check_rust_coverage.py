"""Fail on a Rust line no test runs, unless it is one no test can run.

    cd rust && cargo llvm-cov report --show-missing-lines | python ../scripts/check_rust_coverage.py

The report's TOTAL never reaches 100%: each binary compiles its own copy of a crate, and the
summary counts the copies a binary never calls. The `Uncovered Lines:` list is exact.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

UNREACHABLE = {
    # pyo3 emits a `__pymethod_*__` wrapper per method that Python never calls (the bodies
    # run); `coverage(off)` on the block hides the bodies, not the wrappers.
    ("sqlty-ext/src/lib.rs", "#[pymethods]"),
    # ruff yields IPython escapes only in IPython mode; sqlty parses modules.
    (
        "sqlty-ext/src/outline.rs",
        "Stmt::IpyEscapeCommand(_) => Err(pyo3::exceptions::PySyntaxError::new_err(",
    ),
    ("sqlty-ext/src/outline.rs", '"IPython escape command",'),
    ("sqlty-ext/src/outline.rs", ")),"),
}


def missed(report: str) -> list[tuple[str, int]]:
    found = []
    for entry in report.partition("Uncovered Lines:")[2].splitlines():
        path, _, spans = entry.partition(": ")
        if not spans:
            continue
        for span in spans.split(", "):
            first, _, last = span.partition("-")
            found += [(path, n) for n in range(int(first), int(last or first) + 1)]
    return found


def main() -> int:
    unexpected = []
    for path, n in missed(sys.stdin.read()):
        text = Path(path).read_text().splitlines()[n - 1].strip()
        crate = re.sub(r"^.*?/crates/", "", path)
        if (crate, text) not in UNREACHABLE:
            unexpected.append(f"{crate}:{n}: {text}")
    print("\n".join(unexpected) or "Every reachable Rust line ran.")
    return bool(unexpected)


if __name__ == "__main__":
    sys.exit(main())
