---
name: release
description: Prepare the next sqlty release - pick the version, bump every place with scripts/bump_version.py, draft hand-written release notes from merged PRs, open the bump PR on request.
metadata:
  internal: "true"
---

# Prepare a release

1. Last tag: `gh release list -L 1`. List PRs merged since: `gh pr list --state merged --search "merged:>DATE"`.
2. Version: breaking user-visible change or removed option is at least minor before 1.0. Ask with a questionnaire when unclear.
3. Bump: `uv run python scripts/bump_version.py X.Y.Z`, then `--check`. Add any versioned file it missed to `PLACES`.
4. Notes (the GitHub Release body is hand-written): highlights, breaking changes with migration, features, fixes, perf with numbers, contributors. Leave out tests, CI and dependency-only PRs. Save the draft as a file for review.
5. Branch `release/X.Y.Z`, push. Open the PR only when asked.
6. After merge the maintainer publishes the GitHub Release; check `pypi.yml` and the binary, npm and VS Code jobs, and which channels actually published.

Never tag, publish or upload yourself.
