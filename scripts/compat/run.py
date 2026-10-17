"""Run a comparison source through the harness and write its report.

    uv run --with sqlalchemy==2.0.43 python scripts/compat/run.py sqlalchemy

writes `scripts/compat/reports/sqlalchemy.json` and `.md`.
"""

from __future__ import annotations

import importlib
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from harness import run  # noqa: E402

SOURCES = {"sqlalchemy": "sqlalchemy_rows", "drivers": "drivers"}


def main(argv: list[str]) -> int:
    if len(argv) != 1 or argv[0] not in SOURCES:
        print(f"usage: run.py {{{','.join(SOURCES)}}}", file=sys.stderr)
        return 2
    rows = list(importlib.import_module(SOURCES[argv[0]]).rows())
    report = run(rows)
    report.write(Path(__file__).parent / "reports" / argv[0])
    print(report.markdown().split("\n\n")[1])
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
