#!/usr/bin/env bash
# Rebuild the app up to step N from scratch: ./run.sh 3
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
last="${1:-5}"
for script in "$here"/0*.sh; do
  n="$(basename "$script")"; n="${n%%-*}"
  [ "$((10#$n))" -le "$last" ] || break
  "$script"
done
