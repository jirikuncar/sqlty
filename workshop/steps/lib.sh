# Sourced by every step: shared settings and helpers.
set -euo pipefail

APP="${DUCKDESK_DIR:-$HOME/duckdesk}"
SQLTY_SPEC="${SQLTY_SPEC:-sqlty @ git+https://github.com/jirikuncar/sqlty}"
export GIT_AUTHOR_NAME="${GIT_AUTHOR_NAME:-You}" GIT_AUTHOR_EMAIL="${GIT_AUTHOR_EMAIL:-you@duckdesk.dev}"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME" GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

say() { printf '\n\033[1;33m>> %s\033[0m\n' "$*"; }

commit() {
  git add -A
  git commit -q -m "$1"
  say "committed: $1"
}

as_bob() {
  GIT_AUTHOR_NAME=Bob GIT_AUTHOR_EMAIL=bob@duckdesk.dev \
    GIT_COMMITTER_NAME=Bob GIT_COMMITTER_EMAIL=bob@duckdesk.dev "$@"
}

if [ -d "$APP" ]; then cd "$APP"; fi
