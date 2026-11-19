#!/usr/bin/env bash
# Runs the suite on a copy of the plugin: the runner refuses a plugin with hard links
# (cargo's in rust/target, uv's in .venv), and scaffolds get no network to install sqlty.
# SQLTY_EVAL_SPEC picks the sqlty to test (a wheel, `sqlty==X`); PyPI's latest by default.
# Arguments go to `claude plugin eval`, e.g. `evals/run.sh --case new-query --runs 1`.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
copy="$tmp/sqlty"
mkdir "$copy"
cp -R "$root/.claude-plugin" "$root/skills" "$copy/"
tar -C "$root" --exclude=evals/results -cf - evals | tar -C "$copy" -xf -
# The agent's sandbox hides HOME and the rest of /tmp, so Python and the venv live in the plugin copy.
venv="$copy/.venv"
export UV_PYTHON_INSTALL_DIR="$copy/.python"
uv python install -q 3.14
uv venv -q --managed-python --python 3.14 "$venv"
uv pip install -q --link-mode copy --python "$venv" psycopg "${SQLTY_EVAL_SPEC:-sqlty}"
echo "$venv" > "$copy/evals/_project/venv"
export PATH="$venv/bin:$PATH"  # runs inherit PATH, as from an activated venv
claude plugin eval "$copy" --scaffold --allow-tools Bash Write Edit \
  --output-dir "$root/evals/results/$(date -u +%Y-%m-%dT%H-%M-%SZ)" "$@"
