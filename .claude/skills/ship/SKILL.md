---
name: ship
description: Finish a sqlty branch - merge latest main, run affected tests, fuzz, remeasure against the check budget, self-review with a separate agent, push. Opens a PR only when called with `pr`.
metadata:
  internal: "true"
---

# Ship a branch

Arguments: `pr` opens a pull request after the gates pass; `no-fuzz`, `no-perf` skip a gate (say why in the report).

0. **Spec.** If `plans/` has the branch's plan, check the work against its examples and budget; report any gap.
1. **Sync.** `git fetch origin +refs/heads/main:refs/remotes/origin/main`, list what merged since the branch point, merge it in. Never rebase or force push.
2. **Test.** Only the tests the diff affects; format only modified files. Coverage stays at the gate (`make test-coverage` when Rust or Python lines were added).
3. **Fuzz.** Parser, rule or input-handling changes: run the properties and cargo-fuzz from [DEVELOPMENT.md#fuzzing](../../../DEVELOPMENT.md#fuzzing) with extreme and hostile inputs (deep nesting, huge files, many tables, params, columns, odd unicode). Every crash or mismatch gets a regression test.
4. **Measure.** `make bench-ab ARGS="--scale 1,4"` (main vs the working tree, interleaved, output compared). Blocking: a check slower than a database round trip (AGENTS.md "Check budget"), worse than linear scaling up to 32x input, or CPU/RSS beyond noise. `make bench-ci` passes.
5. **Review.** Start a separate agent with the branch name only and the [`review` skill](../review/SKILL.md). Fix every Important finding; list skipped ones with a reason.
6. **Push.** Stage named paths, check `git status`, commit, `git push -u origin <branch>`.
7. **PR** (only with `pr`). Mirror the PR template, disclose the agent, assign the requester, watch CI and reviews. While auto-merge is on, keep it current with `gh api -X PUT repos/jirikuncar/sqlty/pulls/N/update-branch`.

Report: commit, what main brought in, tests run, fuzz cases and result, perf table vs main, review findings fixed and skipped, PR link.
