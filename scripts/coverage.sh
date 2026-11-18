#!/usr/bin/env bash
# Every Python and Rust test on a coverage-instrumented build, then the coverage gates.
# Needs a nightly toolchain with llvm-tools (for `#[coverage(off)]`), cargo-llvm-cov and
# cargo-nextest; the `pg` tests run when SQLTY_PG_DSN is set.
set -euo pipefail
cd "$(dirname "$0")/.."

export COVERAGE_FILE=$PWD/.coverage
uv run --no-sync coverage erase
eval "$(cd rust && cargo llvm-cov show-env --sh)"
(cd rust && cargo llvm-cov clean --workspace)
uv run --no-sync maturin develop --manifest-path rust/crates/sqlty-ext/Cargo.toml
pytest=(uv run --no-sync pytest --cov --cov-append --cov-fail-under=0 --hypothesis-profile=coverage)
"${pytest[@]}" -m "not timing and not pg"
if [[ -n ${SQLTY_PG_DSN:-} ]]; then
  "${pytest[@]}" -m pg -n 0
fi
"${pytest[@]}" -m timing -n 0 -p no:randomly
(cd rust && uv run --no-sync cargo nextest run --workspace)

uv run --no-sync coverage report
(cd rust && cargo llvm-cov report --show-missing-lines | tee /dev/stderr | awk '/^Uncovered Lines:/ { missed = 1 } END { exit missed }')
