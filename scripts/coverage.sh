#!/usr/bin/env bash
# Each package's own tests on a coverage-instrumented build, gated on the lines they never run.
#
#   scripts/coverage.sh [PACKAGE...]   # python, or a directory of rust/crates; all by default
#
# Python is the package of src/sqlty and the extension crate (rust/crates/sqlty-ext); every
# other crate counts only its own tests. Needs a nightly toolchain with llvm-tools (for
# `#[coverage(off)]`) and cargo-llvm-cov; the `pg` tests run when SQLTY_PG_DSN is set.
set -euo pipefail
cd "$(dirname "$0")/.."
export RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-nightly}
crates=$(ls rust/crates)
# The extension crate is gated with the Python package.
(($#)) || set -- python $(grep -vx sqlty-ext <<<"$crates")
runner=test tests=(test)
if cargo nextest --version >/dev/null 2>&1; then runner=nextest tests=(nextest run); fi

# Fails when the crate in $1 leaves more lines unrun than its floor in
# scripts/coverage-floors.txt (none: 0); asks to lower a floor it is under.
gate() {
  local others missed floor
  others=$(grep -vx "$1" <<<"$crates" | paste -sd'|')
  missed=$(
    (cd rust && cargo llvm-cov report --show-missing-lines --ignore-filename-regex "crates/($others)/") |
      tee /dev/stderr |
      awk '/^Uncovered Lines:/ { listed = 1; next } listed && /: / {
        sub(/^[^:]*: /, "")
        for (i = split($0, ranges, ", "); i; i--) n += split(ranges[i], r, "-") > 1 ? r[2] - r[1] + 1 : 1
      } END { print n + 0 }'
  )
  floor=$(awk -v crate="$1" '$1 == crate { print $2 }' scripts/coverage-floors.txt)
  floor=${floor:-0}
  if ((missed > floor)); then
    echo "::error::$1: $missed lines never run, over its floor of $floor (scripts/coverage-floors.txt)"
    return 1
  elif ((missed < floor)); then
    echo "::warning::$1: $missed lines never run: lower its floor of $floor in scripts/coverage-floors.txt"
  fi
}

python_package() {
  export COVERAGE_FILE=$PWD/.coverage
  uv run --no-sync coverage erase
  (
    eval "$(cd rust && cargo llvm-cov show-env --sh)"
    (cd rust && cargo llvm-cov clean --workspace)
    uv run --no-sync maturin develop --manifest-path rust/crates/sqlty-ext/Cargo.toml
    local pytest=(uv run --no-sync pytest --cov --cov-append --cov-fail-under=0 --hypothesis-profile=coverage)
    "${pytest[@]}" -m "not timing and not pg"
    if [[ -n ${SQLTY_PG_DSN:-} ]]; then
      "${pytest[@]}" -m pg -n 0
    fi
    "${pytest[@]}" -m timing -n 0 -p no:randomly
    (cd rust && uv run --no-sync cargo "${tests[@]}" -p sqlty-core)
    uv run --no-sync coverage report
    gate sqlty-ext
  )
}

crate() {
  local package
  package=$(sed -n 's/^name = "\(.*\)"/\1/p;T;q' "rust/crates/$1/Cargo.toml")
  (cd rust && cargo llvm-cov clean --workspace && cargo llvm-cov $runner --no-report -p "$package")
  gate "$1"
}

for package; do
  if [[ $package == python ]]; then python_package; else crate "$package"; fi
done
