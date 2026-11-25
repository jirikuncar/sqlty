"""The slowest packages and tests of a run, as Markdown, from the JUnit reports in DIR.

    python3 scripts/slowest.py DIR [N]   # N slowest tests, 20 by default

DIR holds one `<package>*.xml` per test run and `times.txt`, `<package> <seconds>` lines
of each package's wall time, build included (scripts/coverage.sh writes both).
"""

import sys
import xml.etree.ElementTree as ET
from collections import defaultdict
from pathlib import Path


def main(directory: Path, top: int) -> None:
    wall = {
        name: int(seconds)
        for name, seconds in (
            line.split() for line in (directory / "times.txt").read_text().splitlines()
        )
    }
    totals: dict[str, float] = defaultdict(float)
    counts: dict[str, int] = defaultdict(int)
    tests = []
    for report in sorted(directory.glob("*.xml")):
        package = next(
            name for name in sorted(wall, key=len, reverse=True) if report.stem.startswith(name)
        )
        for case in ET.parse(report).iter("testcase"):
            if case.find("skipped") is not None:
                continue
            seconds = float(case.get("time", 0))
            totals[package] += seconds
            counts[package] += 1
            name = f"{case.get('classname')}::{case.get('name')}".replace("\n", " ").replace(
                "|", "\\|"
            )
            tests.append((seconds, package, name))
    print(
        "### Slowest packages\n\n| Package | Wall (build + tests) | Tests | Test time (summed) |\n|---|--:|--:|--:|"
    )
    for name in sorted(wall, key=wall.get, reverse=True):
        print(f"| {name} | {wall[name]}s | {counts[name]} | {totals[name]:.1f}s |")
    print(f"\n### Slowest {top} tests\n\n| Time | Package | Test |\n|--:|---|---|")
    for seconds, package, test in sorted(tests, reverse=True)[:top]:
        print(f"| {seconds:.2f}s | {package} | `{test}` |")


if __name__ == "__main__":
    main(Path(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 20)
