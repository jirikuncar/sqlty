#!/usr/bin/env bash
# Build the `sqlty` executable into sqlty._core.data/scripts/, the wheel's scripts.
# Usage: scripts/build_cli.sh [PROFILE]   (default: dev; releases use dist)
set -euo pipefail
cd "$(dirname "$0")/.."
profile=${1:-dev}
cargo build --profile "$profile" --bin sqlty --manifest-path rust/crates/sqlty-ext/Cargo.toml
target=${CARGO_TARGET_DIR:-rust/target}/$([ "$profile" = dev ] && echo debug || echo "$profile")
rm -rf sqlty._core.data
mkdir -p sqlty._core.data/scripts
for exe in sqlty sqlty.exe; do
  if [ -f "$target/$exe" ]; then cp "$target/$exe" sqlty._core.data/scripts/; fi
done
ls sqlty._core.data/scripts/sqlty*
