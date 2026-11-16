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
pytest=(uv run --no-sync pytest --cov --cov-append --cov-fail-under=0)
"${pytest[@]}" -m "not timing and not pg"
# Rust and Python readings of every checked module must agree (`SQLTY_AST`).
subset=(tests/mdtest tests/test_cli.py tests/test_helpers.py tests/test_psycopg.py
  tests/test_driver_values.py tests/test_type_extractor.py tests/test_cross_module.py)
SQLTY_AST=compare "${pytest[@]}" -m "not timing and not pg" "${subset[@]}"
# The deprecated Python passes alone, until 0.8.0 removes them.
SQLTY_AST=python "${pytest[@]}" -m "not timing and not pg" "${subset[@]}" tests/test_source.py \
  tests/test_composed_edges.py tests/test_rust_check.py tests/test_rust_index.py
if [[ -n ${SQLTY_PG_DSN:-} ]]; then
  "${pytest[@]}" -m pg -n 0
fi
"${pytest[@]}" -m timing -n 0 -p no:randomly
(cd rust && cargo nextest run --workspace)

uv run --no-sync coverage report
(cd rust && cargo llvm-cov report --fail-under-lines 95)
