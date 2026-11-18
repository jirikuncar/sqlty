---
name: review
description: Review a sqlty branch or pull request for correctness and best practices, applying REVIEW.md. Use for self-review before asking a human, or when asked to review a branch, diff or PR.
metadata:
  internal: "true"
---

# Review a sqlty branch

Review as someone who did not write the change. Take the branch name, not the
author's summary of it: an agent handed a conclusion tends to confirm it.

## 1. Scope

```bash
git fetch origin +refs/heads/main:refs/remotes/origin/main "+refs/heads/$branch:refs/remotes/origin/$branch"
git switch "$branch"
base=$(git merge-base origin/main HEAD)
git log --oneline "$base"..HEAD
git diff --stat "$base"
git status --short   # nothing untracked or unstaged may hide in the review
```

Read [REVIEW.md](../../../REVIEW.md), the plan in `plans/` the branch follows, and
the issue it fixes. Flag files that should not be in the diff (build output,
`rust/target*`, `.so`, `.wasm`, lockfile churn unrelated to the change).

## 2. Read

Read each changed function whole, with its callers and the other hosts of the same
behavior (`sqlty-cli`, `sqlty-project`, `sqlty-lsp`, `sqlty-wasm`, `editors/vscode`,
the Python passes). A diff hides the code it did not touch but should have.

## 3. Try to break it

For every REVIEW.md "Correctness" item that applies, write the input that would
break the change and run it:

```bash
uv run pytest 'tests/mdtest/<area>/<file>.md::<test>'   # one mdtest
uv run pytest tests/<file>.py -k <name>                   # one test
(cd rust && cargo test -p <crate> <name>)                 # one Rust test
uv run sqlty check <dir>                                  # end to end
```

Put scratch inputs in a temp dir, never in the repo. Run only affected tests; the
author runs the full suite and coverage. Check the claims in commit messages and
the PR body (numbers, "identical output", "no behavior change") against what you
ran.

## 4. Report

Do not edit the branch. Report findings ranked most severe first:

```text
Important | Nit | Unverified  path:line  one-sentence defect
  Input:  the source, settings or command
  Got:    what happens   Expected: what should
  Fix:    the smallest change, reusing existing code where possible
```

Severity and limits follow REVIEW.md. Say in one line what you checked and found
fine, so the author knows what the review covered.
