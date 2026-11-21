---
name: learn
description: After a sqlty PR merges (or a correction in review), turn the maintainer's review comments and corrections into durable rules - REVIEW.md items, AGENTS.md conventions, regression tests or gates - so the same miss does not repeat.
metadata:
  internal: "true"
---

# Learn from a merged PR

Argument: a PR number, or `since <date>` for every PR merged since then.

1. Collect the human signal: review comments and threads (`gh api repos/jirikuncar/sqlty/pulls/N/comments`, `.../reviews`), commits pushed after the first review, and corrections the maintainer gave while the work ran. Skip bot and agent comments.
2. For each, ask what would have caught it before review: a test, a gate (lint, `bench-ci`, coverage, a prek hook), a REVIEW.md "Always check" or "Correctness" item, an AGENTS.md convention, or a step in a skill.
3. Prefer the strongest form that fits: a test or gate over a written rule, a written rule over nothing. Skip one-offs and taste; never duplicate an existing rule, sharpen it instead.
4. Write the changes on `agents/learn-<pr>`: tests fail without the fix they guard; rules are one line with the PR as an example.
5. Push; open a PR only when asked.

Report: table of comment, lesson, where it now lives (path:line or test name), and comments skipped with a reason.
