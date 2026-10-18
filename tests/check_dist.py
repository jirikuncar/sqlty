"""Check that built distributions ship the ``sqlty_magic.pth`` startup hook.

Usage (CI, after building)::

    python3 tests/check_dist.py dist/*.whl dist/*.tar.gz

A wheel must have ``sqlty_magic.pth`` at its root (so it lands in
site-packages) and listed in RECORD; an sdist must contain
``src/sqlty_magic.pth`` so wheels built from it get the file too. maturin
before 1.12.1 resolved ``[tool.maturin] include`` against ``python-source``
and silently left the file out of the wheel. Standard library only.
"""

from __future__ import annotations

import sys
import tarfile
import zipfile

PTH = "sqlty_magic.pth"
SKILL = "sqlty/skill/SKILL.md"


def check(path: str) -> None:
    """Raise ``AssertionError`` if the wheel or sdist at ``path`` lacks the .pth."""
    if path.endswith(".whl"):
        with zipfile.ZipFile(path) as wheel:
            names = wheel.namelist()
            record = next(n for n in names if n.endswith(".dist-info/RECORD"))
            rows = wheel.read(record).decode().splitlines()
        assert PTH in names, f"{path}: {PTH} missing from the wheel root"
        assert any(row.startswith(f"{PTH},") for row in rows), f"{path}: {PTH} missing from RECORD"
        assert SKILL in names, f"{path}: {SKILL} missing, so `sqlty skill` fails"
    elif path.endswith(".tar.gz"):
        with tarfile.open(path) as sdist:
            names = sdist.getnames()
        assert any(n.endswith(f"/src/{PTH}") for n in names), f"{path}: src/{PTH} missing"
        assert any(n.endswith(f"/src/{SKILL}") for n in names), f"{path}: src/{SKILL} missing"
    else:
        raise AssertionError(f"{path}: not a wheel or sdist")


def main(paths: list[str]) -> int:
    if not paths:
        print(__doc__, file=sys.stderr)
        return 2
    for path in paths:
        check(path)
        print(f"ok {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
