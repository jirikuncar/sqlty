---
name: fan-out
description: Split a batch of findings or a large sqlty change into parallel work - architect, coder and integrator agents that measure and reject regressions, or one thread or session per finding.
metadata:
  internal: "true"
---

# Fan out

Arguments: `agents` (default) or `threads`.

## agents

- **Architect** (strongest model): starts from the approved `spec` plan and splits it into small independent tasks with acceptance criteria, files, tests and the expected perf effect. Writes no code.
- **Coders** (fast model): one task each, branch `<area>/<task>` in its own worktree, affected tests only, no PRs.
- **Integrator**: merges task branches one at a time into `<area>/integration`, runs `make bench-ab` against main after each merge (CPU, wall, RSS, output identical). Rejects a task that regresses beyond noise or breaks the check budget or linear scaling, and returns it to its coder with the numbers.

Every brief carries the repo rules: AGENTS.md, REVIEW.md, the check budget, no force push, stage named paths, cargo target dirs under `rust/target/`.

## threads

One thread or session per finding, titled by its goal. Brief each with the finding, evidence, files, rules above, and to report back when a branch is ready. Keep a table here: finding, owner, state. Do not do the work here.

End with a table per task (merged, rejected with reason, skipped), the final A/B vs main, branches pushed, and which are ready for a PR.
