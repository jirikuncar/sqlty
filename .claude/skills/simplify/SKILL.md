---
name: simplify
description: Critical simplification review of sqlty code, docs, CI or setup - find what can be deleted, merged or replaced by a library, ranked by lines saved and risk. Report only.
metadata:
  internal: "true"
---

# Simplification review

Argument: a path, area or `all`.

Look for: code only tests use, logic duplicated across hosts (CLI, LSP, wasm, VS Code), hand-rolled code a dependency already provides, config copied across workflows, docs written twice, public functions with too many parameters, tests that reach internals instead of public APIs, generated files that could be generated in CI.

For each finding: what, where (path:line), what replaces it, lines saved, risk, and what checks it stays correct (tests, coverage, bench). Rank by value over risk.

Report only; each finding a separate branch later. Never trade correctness, the check budget or coverage for fewer lines.
