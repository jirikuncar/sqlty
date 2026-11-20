---
name: fix-ci
description: Diagnose and fix red sqlty CI or a failed release run from a link, run id or pasted log; reproduce locally, fix on a branch, never weaken a guard.
metadata:
  internal: "true"
---

# Fix CI

1. Find the failing job: `gh run view <id> --log-failed`, or the pasted log. Name the job, step and first real error.
2. Classify: our change, main already red (`gh run list --branch main`), flaky (re-run once, compare), environment (runner image, toolchain, token, proxy).
3. Reproduce the failing step locally with the same command; run only that.
4. Fix on a branch (`ci/<what>`). Never skip, xfail, loosen or delete a test, lower a coverage gate, or edit the no-exec harness count to get green.
5. Release runs (`pypi.yml`, binaries, npm, Homebrew): check which artifacts reached PyPI/npm and which tag re-runs still use the old workflow file; say what must be re-published by hand.

Report: cause in one sentence with the log line, fix commit, local evidence, and whether main needs the fix too.
