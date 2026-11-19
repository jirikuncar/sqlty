---
type: regex
pattern: 'sqlty:\s*ignore'
match: not_contains
target:
  source: file
  path: app/reports.py
---
