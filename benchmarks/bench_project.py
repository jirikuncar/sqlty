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
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import synth  # noqa: E402

BASELINE = Path(__file__).parent / "baseline.json"
SIZES = {
    "full": {"files": 10_000, "queries": 3_000, "tables": 500},
    "ci": {"files": 8_000, "queries": 2_400, "tables": 400},
}
# Times of 5 ms or less are noise on a shared runner: they are reported, not budgeted.
MIN_BUDGETED_S = 0.005
ATTEMPTS = 3
# A check must beat the database round trip that types the same query: PostgreSQL Parse and
# Describe over a Unix socket, about 100 us on a machine whose calibration loop takes 20 ms
# (AGENTS.md "Check budget"). A slower runner gets a proportionally larger budget.
ROUND_TRIP_US = 100
ROUND_TRIP_CALIBRATION_S = 0.02
# The string literals synth.py writes queries as.
QUERY = re.compile(r'"(?:--sql\\n)?(?:SELECT|WITH) ')


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


def measure(cmd: list[str], cwd: str, repeats: int) -> dict[str, float]:
    runs = [run_measured(cmd, cwd) for _ in range(repeats)]
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


def file_us(root: Path, schema: str) -> list[float]:
    """Microseconds per query of ``check_source`` on each file with queries, schema cached."""
    from sqlty import core

    settings = core.Settings(schema_sql=schema)
    out = []
    for path in sorted(root.rglob("*.py")):
        source = path.read_text()
        if queries := len(QUERY.findall(source)):
            best = float("inf")
            for _ in range(3):
                t = time.perf_counter()
                core.check_source(source, settings)
                best = min(best, time.perf_counter() - t)
            out.append(best * 1e6 / queries)
    return out


def commit_then_migrate(root: Path) -> str:
    """Commit the project, then add a migration to its schema; the project's path."""
    git = ["git", "-c", "user.name=bench", "-c", "user.email=bench@example.com"]
    shutil.rmtree(root / ".git", ignore_errors=True)  # a kept project, measured again
    for args in (["init", "-q"], ["add", "-A"], ["commit", "-q", "-m", "baseline"]):
        subprocess.run([*git, *args], cwd=root, check=True)
    with (root / "schema.sql").open("a") as schema:
        schema.write(synth.MIGRATION)
    return str(root)


def run(size: str, repeats: int, keep: str | None) -> dict:
    spec = SIZES[size]
    exe = shutil.which("sqlty") or str(Path(sys.executable).parent / "sqlty")
    root = Path(keep) if keep else Path(tempfile.mkdtemp(prefix="sqlty-bench-"))
    try:
        t = time.perf_counter()
        stats = synth.generate(root, **spec)
        gen_s = time.perf_counter() - t
        metrics: dict[str, dict[str, float]] = {}
        proj = str(root)
        metrics["check"] = measure([exe, "check", "-q", proj], proj, repeats)
        metrics["rewrite_check"] = measure([exe, "rewrite", "--check", "-q", proj], proj, repeats)
        empty = root / "empty"
        empty.mkdir()
        for name in ("pyproject.toml", "schema.sql"):
            shutil.copy(root / name, empty / name)
        metrics["check_empty"] = measure([exe, "check", "-q", str(empty)], str(empty), repeats)
        per_file = sorted(file_us(root, (root / "schema.sql").read_text()))
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
        metrics["breaking"] = measure(
            [exe, "breaking", "-q", "--against", "HEAD"], commit_then_migrate(root), min(repeats, 3)
        )
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
            "one_query_us": one_query(spec["tables"]),
            "micro_us": micro(spec["tables"]),
            "file_us_per_query": {
                "median": round(statistics.median(per_file), 1),
                "p99": round(per_file[int(len(per_file) * 0.99)], 1),
                "max": round(per_file[-1], 1),
                "over_round_trip": sum(us > ROUND_TRIP_US for us in per_file),
                "files": len(per_file),
            },
        }
    finally:
        if not keep:
            shutil.rmtree(root, ignore_errors=True)


# A check must beat a database round trip: about 100 us on a machine whose calibration loop
# takes 20 ms (AGENTS.md "Check budget"); a slower runner gets a proportionally larger budget.
ROUND_TRIP_US = 100
ROUND_TRIP_CALIBRATION_S = 0.02
# Turns on once a one-query check fits the budget: until then `one_query_us` is reported only.
ENFORCE_ONE_QUERY = False
ONE_QUERY_SQL = Path(__file__).parent / "one_query.sql"


def one_query(tables: int, rounds: int = 200) -> dict[str, float]:
    """Microseconds for ``check_source`` of a module holding one query of ``one_query.sql``
    (also the Criterion group ``one_query``), as ``sqlty rewrite`` types it, schema loaded
    once: what ``sqlty check`` spends per query."""
    from sqlty import core

    settings = core.Settings(schema_sql=core.load_schema(synth.schema_sql(tables)))
    out: dict[str, float] = {}
    for block in ONE_QUERY_SQL.read_text().split("-- name: ")[1:]:
        name, sql = block.split("\n", 1)
        name = name.strip()
        module = core.rewrite_source(f'q = "--sql\\n{" ".join(sql.split())}"\n', settings)
        if "SQL[" not in module or (found := core.check_source(module, settings)):
            raise SystemExit(f"one_query.{name} is not a typed query without diagnostics: {found}")
        best = float("inf")
        for _ in range(5):
            t = time.perf_counter()
            for _ in range(rounds):
                core.check_source(module, settings)
            best = min(best, (time.perf_counter() - t) / rounds)
        out[name] = round(best * 1e6, 1)
    return out


def one_query_lines(us: dict[str, float], calibration_s: float) -> tuple[list[str], bool]:
    """Each ``one_query_us`` against the round trip budget, and whether one is over it."""
    limit = ROUND_TRIP_US * calibration_s / ROUND_TRIP_CALIBRATION_S
    lines = [
        f"one_query.{name}: {v:.0f} us, {'OVER' if v > limit else 'under'} round trip {limit:.0f} us"
        for name, v in sorted(us.items())
    ]
    return lines, any(v > limit for v in us.values())


def check_one_query(result: dict, tables: int) -> bool:
    """Print ``one_query_us`` against the round trip; ``False`` when ``ENFORCE_ONE_QUERY``
    and a query is over it in each of ``ATTEMPTS`` measurements."""
    us, cal = result["one_query_us"], result["calibration_s"]
    lines, over = one_query_lines(us, cal)
    for attempt in range(2, ATTEMPTS + 1):
        if not (over and ENFORCE_ONE_QUERY):
            break
        print(f"one_query over budget, measuring again ({attempt}/{ATTEMPTS})", file=sys.stderr)
        us, cal = one_query(tables), calibrate()
        lines, over = one_query_lines(us, cal)
    enforced = "budget" if ENFORCE_ONE_QUERY else "reported, not budgeted (ENFORCE_ONE_QUERY)"
    print(f"one query vs round trip, {enforced}:", *lines, sep="\n  ", file=sys.stderr)
    return not (over and ENFORCE_ONE_QUERY)


def flatten(result: dict) -> dict[str, tuple[str, float]]:
    """Budgeted values as ``{name: (kind, value)}``; times are in calibration units."""
    cal = result["calibration_s"]
    flat: dict[str, tuple[str, float]] = {}
    for name, m in result["metrics"].items():
        if m["wall_s"] >= MIN_BUDGETED_S:
            flat[f"{name}.wall"] = ("time", m["wall_s"] / cal)
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


def round_trip(result: dict) -> list[str]:
    """Lines for the checks slower than a database round trip (empty when none is)."""
    limit = ROUND_TRIP_US * result["calibration_s"] / ROUND_TRIP_CALIBRATION_S
    micro_us = result["micro_us"]
    per_query = {
        f"query.{kind}": micro_us[f"analyze_select.{kind}"] + micro_us[f"analyze_params.{kind}"]
        for kind in (
            name.split(".", 1)[1] for name in micro_us if name.startswith("analyze_select.")
        )
    }
    m = result["metrics"]
    check_s = m["check"]["wall_s"] - m["check_empty"]["wall_s"]
    per_query["check.per_query"] = check_s * 1e6 / result["meta"]["queries"]
    return [
        f"{name}: {us:.0f} us > round trip {limit:.0f} us"
        for name, us in sorted(per_query.items())
        if us > limit
    ]


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
    ap.add_argument("--worker-schema", help=argparse.SUPPRESS)
    args = ap.parse_args()
    if args.worker_schema:
        worker_schema_load(args.worker_schema)
        return 0

    result = run(args.size, args.repeats, args.keep)
    key = f"{result['meta']['platform']}-{args.size}"
    path = Path(args.baseline)
    baselines = json.loads(path.read_text()) if path.exists() else {}

    def over(result: dict) -> list[str]:
        failures = round_trip(result)
        if key in baselines:
            failures += compare(result, baselines[key], args.time_tolerance, args.rss_tolerance)
        return failures

    failures = []
    if args.compare:
        failures = over(result)
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
            result = run(args.size, args.repeats, args.keep)
            failures = over(result)
    text = json.dumps(result, indent=2)
    print(text)
    if args.out:
        Path(args.out).write_text(text + "\n")
    if args.compare and not check_one_query(result, SIZES[args.size]["tables"]):
        return 1
    if args.update_baseline:
        baselines[key] = result
        path.write_text(json.dumps(baselines, indent=2, sort_keys=True) + "\n")
        print(f"baseline {key} written to {path}", file=sys.stderr)
        return 0
    if args.compare:
        if failures:
            print("performance budget exceeded:", *failures, sep="\n  ", file=sys.stderr)
            return 1
        if key not in baselines:
            print(
                f"no baseline for {key} in {path}: checked the round trip budget only. "
                "Commit this run's JSON (make bench-baseline) to enable the rest.",
                file=sys.stderr,
            )
        else:
            print(f"within budget versus baseline {key}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
