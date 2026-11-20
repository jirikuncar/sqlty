---
name: triage
description: Triage open sqlty issues - reproduce each on main, fix those that need no maintainer decision, and ask about the rest with researched options in one questionnaire.
metadata:
  internal: "true"
---

# Triage issues

1. `gh issue list --state open`. For each: reproduce on current main with a minimal case; note fixed, still broken, or not reproducible.
2. Sort: fix now (clear expected behavior), needs decision, close candidate (fixed, duplicate, out of scope).
3. Fix-now issues: one branch each (`fix/<issue>`), mdtest first, then the fix; run `ship` without `pr`.
4. Needs-decision issues: research how similar tools handle it, then one questionnaire with 2-4 options each and a recommendation.
5. Nothing is closed, labeled or commented publicly without the maintainer's go-ahead.

Report: table of issue, finding, action, branch; then the questionnaire.
