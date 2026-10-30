#!/usr/bin/env bash
# Rebuild the app up to step N from scratch: ./run.sh 5
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
last="${1:-8}"
for script in "$here"/0*.sh; do
  n="$(basename "$script")"; n="${n%%-*}"
  [ "$((10#$n))" -le "$last" ] || break
  "$script"
done
