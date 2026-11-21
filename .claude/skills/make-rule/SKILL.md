---
name: make-rule
description: Turn a standard the maintainer states into a sqlty rule for agents and reviewers - AGENTS.md, REVIEW.md, the review skill and a gate where it can be checked - then tell the work in progress.
metadata:
  internal: "true"
---

# Make it a rule

Argument: the rule as stated.

1. Make it measurable: the number, the command that checks it, and the source for the number.
2. Write it once where it belongs: conventions in AGENTS.md, severity and checks in REVIEW.md, the step in [review](../review/SKILL.md). Link instead of repeating.
3. Gate it where a machine can: a test, a `bench-ci` budget, a lint, a prek hook. Measure the current state; if main fails it, report that and gate what passes.
4. Commit on `agents/<rule>` and push.
5. Tell the work in progress (threads, sessions, open PRs) in one paragraph: the rule, the number, where it is written.

Report: the rule text, where it lives, the gate, current numbers vs the limit.
