"""Large-project benchmark and CI performance budget.

Generates a synthetic project (see ``synth.py``), measures wall time and peak RSS
of the CLI entry points and of a schema load, times ``analyze_select`` and
``analyze_params`` per query kind, and optionally compares the result with a
committed baseline.

Build the extension in release mode first:
``uv run maturin develop --release --manifest-path rust/crates/sqlty-ext/Cargo.toml``.

Usage:
    uv run python benchmarks/bench_project.py --size full                  # report
    uv run python benchmarks/bench_project.py --size ci --compare          # budget check
    uv run python benchmarks/bench_project.py --size ci --update-baseline  # rewrite baseline
    uv run python benchmarks/bench_project.py --size full --out result.json

Budget: every time is divided by a calibration loop timed on the same machine, so a
slow or fast runner moves the baseline and the result together. A time metric fails
when its normalised value exceeds the baseline by ``--time-tolerance`` (default 50%),
peak RSS when it exceeds the baseline by ``--rss-tolerance`` (default 25%). A run over
budget is measured again, up to three runs in all, and fails only when the last one is too.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import lsp  # noqa: E402
import synth  # noqa: E402

BASELINE = Path(__file__).parent / "baseline.json"
SIZES = {
    "full": {"files": 10_000, "queries": 3_000, "tables": 500},
    "ci": {"files": 8_000, "queries": 2_400, "tables": 400},
}
# The same files with queries in 2% of them, as in an application that adopts sqlty.
SPARSE_QUERIES = 0.02
# Query files of three queries each, beside 200 modules.
SQL_FILES = 2_000
# Modules a pre-commit hook passes, and the queries of the module open in the editor.
PRECOMMIT_FILES = 5
EDITOR_QUERIES = 300
# Times of 5 ms or less are noise on a shared runner: they are reported, not budgeted.
MIN_BUDGETED_S = 0.005
ATTEMPTS = 3


def calibrate() -> float:
    """Seconds for a fixed pure-Python loop (best of 5), the unit times are divided by."""
    best = float("inf")
    for _ in range(5):
        t = time.perf_counter()
        acc = 0
        for i in range(300_000):
            acc = (acc * 31 + i) % 1_000_003
            _ = str(acc)
        best = min(best, time.perf_counter() - t)
    return best


def _rss_mb(ru_maxrss: int) -> float:
    """``ru_maxrss`` is bytes on macOS and KiB on Linux."""
    return ru_maxrss / (1024 * 1024 if sys.platform == "darwin" else 1024)


def run_measured(cmd: list[str], cwd: str) -> tuple[float, float, int]:
    """Run ``cmd``; return ``(wall_seconds, peak_rss_mb, exit_code)`` of that child."""
    start = time.perf_counter()
    proc = subprocess.Popen(cmd, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _, status, usage = os.wait4(proc.pid, 0)
    wall = time.perf_counter() - start
    proc.returncode = os.waitstatus_to_exitcode(status)
    return wall, _rss_mb(usage.ru_maxrss), proc.returncode


def measure(cmd: list[str], cwd: str, repeats: int, fresh: Path | None = None) -> dict[str, float]:
    """Median wall time and peak RSS of ``cmd``; with ``fresh``, each run gets its own
    copy of that directory as ``cwd``, made outside the timing, for commands that write."""
    runs = []
    for _ in range(repeats):
        if fresh is None:
            runs.append(run_measured(cmd, cwd))
            continue
        copy = Path(tempfile.mkdtemp(prefix="sqlty-bench-copy-")) / "p"
        shutil.copytree(fresh, copy)
        try:
            runs.append(run_measured(cmd, str(copy)))
        finally:
            shutil.rmtree(copy.parent, ignore_errors=True)
    return {
        "wall_s": round(statistics.median(r[0] for r in runs), 4),
        "rss_mb": round(max(r[1] for r in runs), 1),
        "exit": runs[0][2],
    }


def worker_schema_load(schema_path: str) -> None:
    """Child entry: parse a schema through one query and print the time it took."""
    from sqlty import core

    sql = Path(schema_path).read_text()
    query = "SELECT id FROM t0005" if "app." in sql else "SELECT id FROM t5"
    t = time.perf_counter()
    cols = core.analyze_select(query, schema_sql=sql)
    first = time.perf_counter() - t
    print(json.dumps({"first_call_s": first, "found": bool(cols)}))


def micro(tables: int, rounds: int = 200) -> dict[str, float]:
    """Microseconds per ``analyze_select``/``analyze_params`` call, per query kind (schema cached)."""
    from sqlty import core

    schema = synth.schema_sql(tables)
    i = min(7, tables - 1)
    out: dict[str, float] = {}
    for name, sql, _fields, _params in synth.KINDS:
        q = sql.format(t=f"t{i:04d}", p=f"t{i - 1:04d}", v=f"v{i:04d}", f=f"f{i:04d}")
        core.analyze_select(q, schema_sql=schema)  # loads and caches the schema
        for fn_name, fn in (
            ("analyze_select", core.analyze_select),
            ("analyze_params", core.analyze_params),
        ):
            best = float("inf")
            for _ in range(5):
                t = time.perf_counter()
                for _ in range(rounds):
                    fn(q, schema_sql=schema)
                best = min(best, (time.perf_counter() - t) / rounds)
            out[f"{fn_name}.{name}"] = round(best * 1e6, 1)
    return out


def run(size: str, repeats: int, keep: str | None, exe: str) -> dict:
    spec = SIZES[size]
    root = Path(keep) if keep else Path(tempfile.mkdtemp(prefix="sqlty-bench-"))
    try:
        t = time.perf_counter()
        stats = synth.generate(root, **spec)
        gen_s = time.perf_counter() - t
        metrics: dict[str, dict[str, float]] = {}
        proj = str(root)
        metrics["check"] = measure([exe, "check", "-q", proj], proj, repeats)
        metrics["rewrite_check"] = measure([exe, "rewrite", "--check", "-q", proj], proj, repeats)
        metrics.update(scenarios(exe, root, spec, repeats))
        for name, text in (
            ("schema_load", (root / "schema.sql").read_text()),
            ("schema_load_unresolved_alters", synth.unresolved_alter_schema(2000)),
            # The name index completion reads is pointers into the catalog: O(N), no copy.
            ("schema_load_5000", synth.schema_sql(5000)),
        ):
            sp = root / f"{name}.sql"
            sp.write_text(text)
            m = measure([sys.executable, __file__, "--worker-schema", str(sp)], proj, repeats)
            metrics[name] = m
        return {
            "meta": {
                "size": size,
                **stats,
                "platform": platform.system(),
                "machine": platform.machine(),
                "python": platform.python_version(),
                "generate_s": round(gen_s, 2),
            },
            "calibration_s": round(calibrate(), 5),
            "metrics": metrics,
            "micro_us": micro(spec["tables"]),
        }
    finally:
        if not keep:
            shutil.rmtree(root, ignore_errors=True)


def scenarios(exe: str, root: Path, spec: dict[str, int], repeats: int) -> dict:
    """The runs users wait on besides a full check: a pre-commit hook, a rewrite that
    writes, an editor, query files and a project where few files hold queries."""
    out: dict[str, dict[str, float]] = {}
    pkg = root / synth.PKG
    changed = [
        str(f.relative_to(root)) for f in sorted(pkg.glob("mod*.py")) if "SQL[" in f.read_text()
    ][:PRECOMMIT_FILES]
    out["precommit"] = measure([exe, "check", "-q", *changed], str(root), repeats)
    out["rewrite"] = measure([exe, "rewrite", "-q", "."], "", repeats, fresh=root)

    editor = pkg / "editor.py"
    synth.write_module(editor, queries=EDITOR_QUERIES, tables=spec["tables"])
    table = "t" + editor.read_text().split("FROM t", 1)[1][:4]
    try:
        out.update(lsp.measure(exe, root, editor, table))
    finally:
        editor.unlink()

    for name, extra in (
        ("sparse", {"queries": int(spec["files"] * SPARSE_QUERIES)}),
        ("sql_files", {"files": 200, "queries": 50, "sql_files": SQL_FILES}),
    ):
        other = Path(tempfile.mkdtemp(prefix=f"sqlty-bench-{name}-"))
        try:
            synth.generate(other, **{**spec, **extra})
            out[name] = measure([exe, "check", "-q", "."], str(other), repeats)
        finally:
            shutil.rmtree(other, ignore_errors=True)
    return out


def flatten(result: dict) -> dict[str, tuple[str, float]]:
    """Budgeted values as ``{name: (kind, value)}``; times are in calibration units."""
    cal = result["calibration_s"]
    flat: dict[str, tuple[str, float]] = {}
    for name, m in result["metrics"].items():
        if m["wall_s"] >= MIN_BUDGETED_S:
            flat[f"{name}.wall"] = ("time", m["wall_s"] / cal)
        if "rss_mb" in m:
            flat[f"{name}.rss"] = ("rss", m["rss_mb"])
    # Per-kind microsecond timings are too noisy to budget one by one: budget the sums.
    for fn in ("analyze_select", "analyze_params"):
        total_us = sum(us for name, us in result["micro_us"].items() if name.startswith(fn + "."))
        flat[f"micro.{fn}.total"] = ("time", total_us * 1e-6 / cal)
    return flat


def compare(result: dict, baseline: dict, time_tol: float, rss_tol: float) -> list[str]:
    """Human-readable lines for the metrics over budget (empty when within budget)."""
    new, old = flatten(result), flatten(baseline)
    failures = []
    for name, (kind, base) in sorted(old.items()):
        if name not in new:
            continue
        value = new[name][1]
        limit = base * (1 + (time_tol if kind == "time" else rss_tol))
        if value > limit:
            failures.append(
                f"{name}: {value:.4g} > budget {limit:.4g} (baseline {base:.4g}, +{value / base - 1:.0%})"
            )
    return failures


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--size", choices=SIZES, default="full")
    ap.add_argument(
        "--repeats", type=int, default=5, help="runs per CLI metric (median wall, max RSS)"
    )
    ap.add_argument("--out", help="write the result JSON here")
    ap.add_argument("--keep", help="generate into this directory and keep it")
    ap.add_argument(
        "--compare", action="store_true", help="fail when over budget versus baseline.json"
    )
    ap.add_argument(
        "--update-baseline", action="store_true", help="store this run in baseline.json"
    )
    ap.add_argument("--baseline", default=str(BASELINE))
    ap.add_argument("--time-tolerance", type=float, default=0.5)
    ap.add_argument("--rss-tolerance", type=float, default=0.25)
    ap.add_argument(
        "--exe",
        default=shutil.which("sqlty") or str(Path(sys.executable).parent / "sqlty"),
        help="the sqlty to run: the wheel's script (default) or a native binary",
    )
    ap.add_argument("--worker-schema", help=argparse.SUPPRESS)
    args = ap.parse_args()
    if args.worker_schema:
        worker_schema_load(args.worker_schema)
        return 0

    args.exe = str(Path(shutil.which(args.exe) or args.exe).absolute())
    result = run(args.size, args.repeats, args.keep, args.exe)
    key = f"{result['meta']['platform']}-{args.size}"
    path = Path(args.baseline)
    baselines = json.loads(path.read_text()) if path.exists() else {}
    failures = []
    if args.compare and key in baselines:
        failures = compare(result, baselines[key], args.time_tolerance, args.rss_tolerance)
        # Another job on a shared runner can slow one run: measure again before failing.
        for attempt in range(2, ATTEMPTS + 1):
            if not failures:
                break
            print(
                f"over budget, measuring again ({attempt}/{ATTEMPTS}):",
                *failures,
                sep="\n  ",
                file=sys.stderr,
            )
            result = run(args.size, args.repeats, args.keep, args.exe)
            failures = compare(result, baselines[key], args.time_tolerance, args.rss_tolerance)
    text = json.dumps(result, indent=2)
    print(text)
    if args.out:
        Path(args.out).write_text(text + "\n")
    if args.update_baseline:
        baselines[key] = result
        path.write_text(json.dumps(baselines, indent=2, sort_keys=True) + "\n")
        print(f"baseline {key} written to {path}", file=sys.stderr)
        return 0
    if args.compare:
        if key not in baselines:
            print(
                f"no baseline for {key} in {path}: nothing to compare. "
                "Commit this run's JSON (make bench-baseline) to enable the budget.",
                file=sys.stderr,
            )
            return 0
        if failures:
            print("performance budget exceeded:", *failures, sep="\n  ", file=sys.stderr)
            return 1
        print(f"within budget versus baseline {key}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
