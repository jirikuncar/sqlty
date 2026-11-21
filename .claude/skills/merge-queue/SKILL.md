---
name: merge-queue
description: Keep the maintainer's sqlty PRs that already have auto-merge enabled up to date with main, one at a time, until they merge.
metadata:
  internal: "true"
---

# Merge queue

1. List open PRs with auto-merge set (`gh api repos/jirikuncar/sqlty/pulls/N --jq .auto_merge`). Never enable or disable auto-merge.
2. Order: stacked parents first, then oldest. Retarget a stacked PR to main once its parent merges.
3. Take one PR: if behind main, `gh api -X PUT repos/jirikuncar/sqlty/pulls/N/update-branch`. Never merge main locally and push.
4. Wait for CI and the merge (subscribe to PR events, no polling loop). A failure stops that PR: report the job and first error, move to the next.
5. Repeat until the queue is empty.

Report per PR: merged at, or blocked with reason.
