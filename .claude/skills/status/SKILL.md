---
name: status
description: Report where sqlty work stands - what is left, which branches are ready for a PR, what merged to main meanwhile, PR CI and review state, and what needs a decision. Read-only.
metadata:
  internal: "true"
---

# Status

Scope: the current task by default; `all` covers every branch and PR of the requester.

1. Fetch main and the work's branches by name (explicit refspecs). List commits merged to main since the last report; flag those touching files this work changes.
2. Per branch: pushed, ahead/behind main, last test and bench result, open findings.
3. Per PR (`gh pr list --author @me`, `gh pr checks N`): failing jobs with the first error line, unanswered review comments, auto-merge, behind main.
4. Earlier findings and checklist items not yet done.

Output in this order, one line per item:

- **Needs you** - decisions and approvals, as a questionnaire when there are options.
- **Ready for PR** - passed tests, perf and self-review.
- **In progress** - with the next step.
- **Main** - merged since last check that affects this work.

Do not start work. End with the single most useful next step.
