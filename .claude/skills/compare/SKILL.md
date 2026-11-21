---
name: compare
description: Compare sqlty with a similar tool or project (named, or found by search) and report gaps, with each claim checked against its source code, docs or a run.
metadata:
  internal: "true"
---

# Compare with another tool

Argument: a tool, repository or topic. With a topic, find the 3-5 closest open-source tools first.

1. Read the tool's docs and source; install and run it on the same inputs as sqlty where possible.
2. Run sqlty on the same cases (`uv run sqlty check`, the playground examples).
3. Table: capability, the tool, sqlty, evidence (link to source line, doc or command output).
4. Gaps sqlty should close, ranked by user value and cost, each with a minimal reproducer; things sqlty does better; things to ignore and why.

Report only. No branches, issues or comments unless asked. Cite every claim; mark unverified ones.
