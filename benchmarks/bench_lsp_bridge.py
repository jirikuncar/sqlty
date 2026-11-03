"""Latency of the language server's bridge: one check per keystroke on a large project.

Stage 2 of ``plans/2026-10-28-vscode-extension.md``. Generates the project of
``bench_project.py``, loads it into ``sqlty._lsp_bridge.Workspace`` and reports, in ms:

- ``load``: reading the project, its config, schema and helpers, once per server start;
- ``check``: a check of a changed buffer, for a generated module and for a large one;
- ``helper edit``: a check after an edit to the module that defines the helpers;
- ``gil lag``: how late a thread that sleeps 1 ms wakes while checks run, the wait of a
  protocol loop written in Python (a pygls server);
- ``sequential`` and ``parallel``: two checks of the large module, one after the other
  and at once: the Python passes hold the GIL, so checks do not overlap.

Build the extension in release mode first:
``uv run maturin develop --release --manifest-path rust/crates/sqlty-ext/Cargo.toml``.

Usage:
    uv run python benchmarks/bench_lsp_bridge.py
    uv run python benchmarks/bench_lsp_bridge.py --files 2000 --queries 600 --tables 100
"""

from __future__ import annotations

import argparse
import shutil
import statistics
import sys
import tempfile
import threading
import time
from collections.abc import Callable
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import synth  # noqa: E402

from sqlty._lsp_bridge import Workspace  # noqa: E402

LARGE_MODULES = 50


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--files", type=int, default=8_000)
    parser.add_argument("--queries", type=int, default=2_400)
    parser.add_argument("--tables", type=int, default=400)
    parser.add_argument("--rounds", type=int, default=50)
    args = parser.parse_args()
    root = Path(tempfile.mkdtemp(prefix="sqlty-bridge-"))
    try:
        synth.generate(root, files=args.files, queries=args.queries, tables=args.tables)
        package = root / synth.PKG
        with_queries = sorted(package.glob("mod*.py"), key=lambda p: -p.read_text().count("SQL["))
        small = with_queries[0]
        large = package / "large.py"
        large.write_text(_concatenate(with_queries[:LARGE_MODULES]))
        print(f"{args.files} files, {args.queries} queries, {args.tables} tables")
        print(
            f"small: {small.read_text().count('SQL[')} queries; large: {large.read_text().count('SQL[')}"
        )

        start = time.perf_counter()
        workspace = Workspace(str(root))
        print(f"load         {1000 * (time.perf_counter() - start):8.1f}")

        for name, path in (("small", small), ("large", large)):
            body = path.read_text()
            texts = [f"{body}\n# {i}\n" for i in range(args.rounds)]
            _report(f"check {name}", _checks(workspace, path, texts))

        db = package / "db.py"
        helpers = db.read_text()
        texts = [f"{helpers}\n\ndef helper_{i}() -> None: ...\n" for i in range(5)]
        _report("helper edit", _checks(workspace, db, texts))

        text = large.read_text()
        lags: list[float] = []
        done = threading.Event()

        def probe() -> None:
            while not done.is_set():
                start = time.perf_counter()
                time.sleep(0.001)
                lags.append(1000 * (time.perf_counter() - start) - 1)

        thread = threading.Thread(target=probe)
        thread.start()
        for i in range(args.rounds):
            workspace.check(str(large), f"{text}\n# lag {i}\n")
        done.set()
        thread.join()
        _report("gil lag", lags)

        copy = package / "large_copy.py"

        def both(i: int) -> None:
            workspace.check(str(large), f"{text}\n# both {i}\n")
            workspace.check(str(copy), f"{text}\n# both {i}\n")

        def pair(i: int) -> None:
            other = threading.Thread(
                target=workspace.check, args=(str(copy), f"{text}\n# pair {i}\n")
            )
            other.start()
            workspace.check(str(large), f"{text}\n# pair {i}\n")
            other.join()

        _report("sequential", [_time(lambda i=i: both(i)) for i in range(args.rounds)])
        _report("parallel", [_time(lambda i=i: pair(i)) for i in range(args.rounds)])
    finally:
        shutil.rmtree(root, ignore_errors=True)


def _concatenate(paths: list[Path]) -> str:
    lines = ["from __future__ import annotations"]
    for path in paths:
        lines += [line for line in path.read_text().splitlines() if "__future__" not in line]
    return "\n".join(lines) + "\n"


def _checks(workspace: Workspace, path: Path, texts: list[str]) -> list[float]:
    return [_time(lambda text=text: workspace.check(str(path), text)) for text in texts]


def _time(work: Callable[[], object]) -> float:
    start = time.perf_counter()
    work()
    return 1000 * (time.perf_counter() - start)


def _report(name: str, times: list[float]) -> None:
    times = sorted(times)
    p95 = times[int(0.95 * (len(times) - 1))]
    print(f"{name:12} {statistics.median(times):8.1f} p95 {p95:8.1f} max {times[-1]:8.1f}")


if __name__ == "__main__":
    main()
