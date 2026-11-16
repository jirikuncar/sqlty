"""Check that built distributions ship the ``sqlty_magic.pth`` startup hook.

Usage (CI, after building)::

    python3 tests/check_dist.py dist/*.whl dist/*.tar.gz

A wheel must have ``sqlty_magic.pth`` at its root (so it lands in
site-packages) and listed in RECORD; an sdist must contain
``src/sqlty_magic.pth`` so wheels built from it get the file too. maturin
before 1.12.1 resolved ``[tool.maturin] include`` against ``python-source``
and silently left the file out of the wheel. The sdist must also contain the
Agent Skills of ``skills/``, which the extension built from it compiles in for
``sqlty skill``. A wheel must also ship the ``sqlty`` executable as its script
(``scripts/build_cli.sh``), since no entry point stands in for it; a wheel built from
the sdist has none (``--without-executable``). Standard library only.
"""

from __future__ import annotations

import sys
import tarfile
import zipfile

PTH = "sqlty_magic.pth"
SKILL = "skills/sqlty/SKILL.md"


def check(path: str, *, executable: bool = True) -> None:
    """Raise ``AssertionError`` if the wheel or sdist at ``path`` lacks the .pth."""
    if path.endswith(".whl"):
        with zipfile.ZipFile(path) as wheel:
            names = wheel.namelist()
            record = next(n for n in names if n.endswith(".dist-info/RECORD"))
            rows = wheel.read(record).decode().splitlines()
        assert PTH in names, f"{path}: {PTH} missing from the wheel root"
        assert any(row.startswith(f"{PTH},") for row in rows), f"{path}: {PTH} missing from RECORD"
        scripts = {n.split("/", 2)[-1] for n in names if ".data/scripts/" in n}
        assert not executable or scripts & {"sqlty", "sqlty.exe"}, (
            f"{path}: the sqlty executable is missing"
        )
    elif path.endswith(".tar.gz"):
        with tarfile.open(path) as sdist:
            names = sdist.getnames()
        assert any(n.endswith(f"/src/{PTH}") for n in names), f"{path}: src/{PTH} missing"
        assert any(n.endswith(f"/{SKILL}") for n in names), (
            f"{path}: {SKILL} missing, so the build fails"
        )
    else:
        raise AssertionError(f"{path}: not a wheel or sdist")


def main(paths: list[str]) -> int:
    executable = "--without-executable" not in paths
    paths = [path for path in paths if path != "--without-executable"]
    if not paths:
        print(__doc__, file=sys.stderr)
        return 2
    for path in paths:
        check(path, executable=executable)
        print(f"ok {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
