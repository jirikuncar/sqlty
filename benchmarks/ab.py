"""Interleaved A/B of `sqlty check` between a base ref and the working tree.

Builds the native CLI for both (release, shared target dir under rust/target/ab),
copies each binary to a fresh file, generates the synthetic project at every
scale, then alternates base and head runs. Reports median wall, CPU and max RSS,
microseconds of CPU per query at each scale, and whether the output is identical.

    make bench-ab                         # main vs working tree, ci size
    make bench-ab BASE=v0.8.0 ARGS="--scale 1,4 --repeats 9"
    uv run python benchmarks/ab.py --project ../app --base main

Exits 1 when output differs, CPU or RSS regresses past --tolerance, or CPU per
query grows faster than the input (worse than linear).
"""

from __future__ import annotations

import argparse
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import synth
from bench_project import SIZES

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "rust" / "target" / "ab"


def git(*args: str, cwd: Path = ROOT) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


def build(src: Path, label: str) -> Path:
    env = {**os.environ, "CARGO_TARGET_DIR": str(WORK / "target")}
    subprocess.run(
        ["cargo", "build", "--release", "--quiet", "-p", "sqlty-cli", "--bin", "sqlty"],
        cwd=src / "rust",
        env=env,
        check=True,
    )
    out = WORK / "bin" / label
    out.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(WORK / "target" / "release" / "sqlty", out)
    out.chmod(0o755)
    return out


def base_binary(ref: str) -> tuple[Path, str]:
    sha = git("rev-parse", "--short", ref)
    tree = WORK / f"src-{sha}"
    if not tree.exists():
        git("worktree", "add", "--detach", str(tree), sha)
    return build(tree, f"base-{sha}"), sha


def run(exe: Path, project: Path) -> tuple[float, float, float, bytes]:
    t = time.perf_counter()
    proc = subprocess.Popen(
        [str(exe), "check", "--output-format", "json", str(project)],
        cwd=project,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    out = proc.stdout.read() if proc.stdout else b""
    _, _, usage = os.wait4(proc.pid, 0)
    wall = time.perf_counter() - t
    return wall, usage.ru_utime + usage.ru_stime, usage.ru_maxrss / 1024, out


def compare(base: Path, head: Path, project: Path, repeats: int) -> dict[str, dict[str, float]]:
    runs: dict[str, list[tuple[float, float, float]]] = {"base": [], "head": []}
    outputs: dict[str, set[bytes]] = {"base": set(), "head": set()}
    run(base, project), run(head, project)
    for i in range(repeats):
        order = (("base", base), ("head", head)) if i % 2 == 0 else (("head", head), ("base", base))
        for name, exe in order:
            wall, cpu, rss, out = run(exe, project)
            runs[name].append((wall, cpu, rss))
            outputs[name].add(out)
    result = {
        name: {
            "wall_s": statistics.median(r[0] for r in rs),
            "cpu_s": statistics.median(r[1] for r in rs),
            "rss_mb": max(r[2] for r in rs),
        }
        for name, rs in runs.items()
    }
    result["identical"] = {
        "value": float(outputs["base"] == outputs["head"] and len(outputs["head"]) == 1)
    }
    return result


def delta(base: float, head: float) -> str:
    return f"{(head - base) / base * 100:+.1f}%" if base else "n/a"


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--base", default="origin/main", help="git ref to compare against")
    ap.add_argument("--size", choices=SIZES, default="ci")
    ap.add_argument("--scale", default="1", help="comma-separated multiples of --size, e.g. 1,4")
    ap.add_argument("--project", type=Path, help="a real project instead of the synthetic one")
    ap.add_argument("--repeats", type=int, default=7)
    ap.add_argument("--tolerance", type=float, default=0.10, help="allowed CPU and RSS regression")
    args = ap.parse_args()

    base, sha = base_binary(args.base)
    head = build(
        ROOT,
        f"head-{git('rev-parse', '--short', 'HEAD')}{'-dirty' if git('status', '--porcelain') else ''}",
    )
    print(f"base {args.base} ({sha}) vs head (working tree)", file=sys.stderr)

    failures: list[str] = []
    per_query: dict[str, list[tuple[int, float]]] = {"base": [], "head": []}
    scales = [int(s) for s in args.scale.split(",")]
    for scale in [0] if args.project else scales:
        with tempfile.TemporaryDirectory(prefix="sqlty-ab-") as tmp:
            if args.project:
                project, label, queries = args.project.resolve(), str(args.project), 0
            else:
                spec = {k: v * scale for k, v in SIZES[args.size].items()}
                project, label = Path(tmp), f"{args.size} x{scale}"
                queries = synth.generate(project, **spec)["queries"]
            r = compare(base, head, project, args.repeats)
        print(f"\n{label}" + (f" ({queries} queries)" if queries else ""))
        print(f"{'':8}{'base':>10}{'head':>10}{'delta':>9}")
        for key in ("wall_s", "cpu_s", "rss_mb"):
            b, h = r["base"][key], r["head"][key]
            print(f"{key:8}{b:10.3f}{h:10.3f}{delta(b, h):>9}")
            if key != "wall_s" and h > b * (1 + args.tolerance):
                failures.append(f"{label}: {key} {delta(b, h)}")
        if not r["identical"]["value"]:
            failures.append(f"{label}: output differs from base")
        print(f"output  {'identical' if r['identical']['value'] else 'DIFFERS'}")
        if queries:
            for name in per_query:
                per_query[name].append((scale, r[name]["cpu_s"] / queries * 1e6))

    if len(scales) > 1 and not args.project:
        print("\nCPU us per query by scale")
        for name, points in per_query.items():
            print(f"{name:8}" + "".join(f"{f'x{s}':>8}{us:8.1f}" for s, us in points))
        (s0, u0), (s1, u1) = per_query["head"][0], per_query["head"][-1]
        if u1 > u0 * (1 + args.tolerance) * 1.25:
            failures.append(
                f"head per-query CPU grows {u1 / u0:.2f}x from x{s0} to x{s1}: worse than linear"
            )

    for f in failures:
        print(f"FAIL {f}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
