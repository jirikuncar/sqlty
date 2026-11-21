---
name: spec
description: Write a sqlty plan before code - acceptance criteria as mdtests, affected hosts, expected perf effect and risks - in plans/, then wait for the maintainer's approval. ship and fan-out check work against it.
metadata:
  internal: "true"
---

# Spec first

Argument: an issue number or a one-line goal.

1. Read the issue, the code it touches and how similar tools handle it (`compare` when the design is open).
2. Write `plans/YYYY-MM-DD-<slug>.md` (date of writing):
   - **Goal** - one paragraph, the user-visible change.
   - **Examples** - the mdtests that will prove it, written out with expected assertions (`# error:`, `-- columns:`, `# rewritten:`), including the edge cases and what must not change.
   - **Hosts** - CLI, `sqlty server`, wasm playground, VS Code: which change and how they stay in agreement.
   - **Budget** - expected effect on per-query check time and scaling, and how it will be measured (`make bench-ab`).
   - **Out of scope**, **Risks**, **Open questions** (as a questionnaire).
3. Commit the plan alone on the feature branch, push, and stop. Implementation starts only after the maintainer approves the examples.
4. When the work ships, delete the plan (AGENTS.md); move what is left to `plans/2026-02-13-future.md` or an issue.
