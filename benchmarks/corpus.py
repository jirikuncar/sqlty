"""``sqlty check`` on real repositories, pinned to a commit.

Each run clones the repositories (shallow, at their pinned commit), writes the
``sqlty.toml`` given here and records wall time, peak RSS and the diagnostics per rule.
Diagnostic counts are compared with ``corpus.json``: a change is reported, not failed,
because a fix and a regression both move them. A crash or an ``internal-error`` fails.

Usage:
    uv run python benchmarks/corpus.py                 # compare with corpus.json
    uv run python benchmarks/corpus.py --update        # rewrite corpus.json
    uv run python benchmarks/corpus.py --summary FILE  # also write a Markdown table
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

EXPECTED = Path(__file__).parent / "corpus.json"

CORPUS = {
    # No queries: what sqlty costs on a large codebase that does not use it.
    "django": {
        "url": "https://github.com/django/django",
        "commit": "03b95b8fbab990ebbe399a4676e4ad581fe9acfd",
        "config": 'dialect = "postgres"\n',
    },
    # sqlc query files over a real migration-built schema.
    "goalert": {
        "url": "https://github.com/target/goalert",
        "commit": "5ed6155988586be91209df15d5cacb6ef8008330",
        "config": (
            'dialect = "postgres"\n'
            'schema = ["migrate/schema.sql", "swo/swoinfo/pgtables.sql", "swo/swosync/changelog.sql"]\n'
            'queries = ["*/queries.sql", "*/*/queries.sql"]\n'
        ),
    },
    # Raw SQL through postgres.py (`db.one`, `db.all`) on an attribute, not a typed name.
    "liberapay": {
        "url": "https://github.com/liberapay/liberapay.com",
        "commit": "cf2592f8fd158be38b525cf259ff875b33d5ac12",
        "config": (
            'dialect = "postgres"\n'
            'schema = "sql/schema.sql"\n'
            "\n"
            "[[functions]]\n"
            'import-path = "postgres.Postgres.one"\n'
            'query-param = "sql"\n'
            "\n"
            "[[functions]]\n"
            'import-path = "postgres.Postgres.all"\n'
            'query-param = "sql"\n'
        ),
    },
}


def clone(url: str, commit: str, into: Path) -> None:
    def git(*args: str) -> None:
        # Git LFS objects (liberapay's fonts) need credentials and are not code.
        env = {**os.environ, "GIT_LFS_SKIP_SMUDGE": "1"}
        done = subprocess.run(["git", *args], cwd=into, env=env, capture_output=True, text=True)
        if done.returncode:
            raise RuntimeError(f"git {' '.join(args)}: {done.stderr.strip()}")

    into.mkdir(parents=True)
    git("init", "-q")
    git("fetch", "-q", "--depth", "1", url, commit)
    git("checkout", "-q", "FETCH_HEAD")


def head(root: Path) -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True
    ).stdout.strip()


def check(exe: str, root: Path) -> dict:
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        start = time.perf_counter()
        proc = subprocess.Popen(
            [exe, "check", "--exit-zero", "--output-format", "json", "."],
            cwd=root,
            stdout=out,
            stderr=err,
        )
        _, status, usage = os.wait4(proc.pid, 0)
        wall = time.perf_counter() - start
        out.seek(0)
        err.seek(0)
        if code := os.waitstatus_to_exitcode(status):
            raise RuntimeError(f"sqlty check exited {code}: {err.read().decode()[-2000:]}")
        found = json.loads(out.read())
    rss = usage.ru_maxrss / (1024 * 1024 if sys.platform == "darwin" else 1024)
    rules = collections.Counter(d["rule"] for d in found)
    if rules["internal-error"]:
        raise RuntimeError(f"internal-error in {root}: {found}")
    return {"wall_s": round(wall, 3), "rss_mb": round(rss, 1), "rules": dict(sorted(rules.items()))}


def summary(results: dict, expected: dict) -> str:
    """A Markdown table of each repository, with the rule counts that changed."""
    lines = [
        "| Repository | Wall (s) | Peak RSS (MB) | Diagnostics | Changed |",
        "|---|--:|--:|--:|---|",
    ]
    for name, r in results.items():
        if "error" in r:
            lines.append(f"| {name} | | | | failed: {r['error']} |")
            continue
        old = expected.get(name, {}).get("rules", {})
        changed = [] if expected.get(name, {}).get("commit") == r["commit"] else ["new commit"]
        changed += [
            f"`{rule}` {old.get(rule, 0)} → {r['rules'].get(rule, 0)}"
            for rule in sorted({*old, *r["rules"]})
            if old.get(rule, 0) != r["rules"].get(rule, 0)
        ]
        total = sum(r["rules"].values())
        lines.append(f"| {name} | {r['wall_s']} | {r['rss_mb']} | {total} | {', '.join(changed)} |")
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--exe", default=shutil.which("sqlty") or str(Path(sys.executable).parent / "sqlty")
    )
    ap.add_argument("--update", action="store_true", help="rewrite corpus.json with this run")
    ap.add_argument("--summary", help="append a Markdown table here, such as $GITHUB_STEP_SUMMARY")
    ap.add_argument("--keep", help="clone into this directory and keep it")
    args = ap.parse_args()
    args.exe = str(Path(shutil.which(args.exe) or args.exe).absolute())

    base = Path(args.keep) if args.keep else Path(tempfile.mkdtemp(prefix="sqlty-corpus-"))
    try:
        results = {}
        for name, repo in CORPUS.items():
            root = base / name
            try:
                if root.exists() and head(root) != repo["commit"]:
                    shutil.rmtree(root)
                if not root.exists():
                    clone(repo["url"], repo["commit"], root)
                (root / "sqlty.toml").write_text(repo["config"])
                results[name] = {"commit": repo["commit"], **check(args.exe, root)}
            except (RuntimeError, subprocess.CalledProcessError) as error:
                results[name] = {"commit": repo["commit"], "error": str(error)}
    finally:
        if not args.keep:
            shutil.rmtree(base, ignore_errors=True)

    expected = json.loads(EXPECTED.read_text()) if EXPECTED.exists() else {}
    table = summary(results, expected)
    print(json.dumps(results, indent=2))
    print(table, file=sys.stderr)
    if args.summary:
        with open(args.summary, "a") as f:
            f.write("## sqlty check on real repositories\n\n" + table)
    failed = [name for name, r in results.items() if "error" in r]
    if args.update and not failed:
        counts = {n: {"commit": r["commit"], "rules": r["rules"]} for n, r in results.items()}
        EXPECTED.write_text(json.dumps(counts, indent=2) + "\n")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
