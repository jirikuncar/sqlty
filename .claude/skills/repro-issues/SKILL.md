---
name: repro-issues
description: Turn reported sqlty cases (an artifact, report or list) into minimal standalone reproducers on current main, check none exposes private data, and file issues only when asked.
metadata:
  internal: "true"
---

# Reproduce and file

1. Per case: a minimal standalone reproducer (schema, Python file, `pyproject.toml`, command) that runs on current main. SQL strings need the `--sql\n` marker.
2. Mark each: reproduced, fixed on main (name the PR), by design (link the doc or issue), not reproduced.
3. Strip anything private: company names, internal hosts, real schemas, emails, tokens.
4. Search existing issues for duplicates.
5. File one issue per reproduced case only when asked: title as the wrong behavior, reproducer, got vs expected, version.

Report: table of case, status, issue link or reason not filed.
