#!/usr/bin/env bash
# Run comparison sources with the versions pinned in scripts/compat/pyproject.toml.
#
#   scripts/compat/run.sh                 # every source
#   scripts/compat/run.sh sqlalchemy django
#
# The drivers, sqlglot and pony sources read SQLTY_PG_DSN and SQLTY_MYSQL_DSN.
set -euo pipefail
cd "$(dirname "$0")/../.."
sources=("$@")
if [ ${#sources[@]} -eq 0 ]; then
    sources=(sqlalchemy drivers sqlglot django tortoise sqlmodel piccolo ormar peewee pony sqlobject)
fi
requirements=$(mktemp)
trap 'rm -f "$requirements"' EXIT
for source in "${sources[@]}"; do
    uv export --project scripts/compat --only-group "$source" --no-hashes --frozen --no-emit-project \
        --quiet > "$requirements"
    echo "== $source"
    uv run --no-sync --with-requirements "$requirements" python scripts/compat/run.py "$source"
done
