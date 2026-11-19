---
type: regex
pattern: '--sql[ \t]+\S'
match: not_contains
target:
  source: file
  path: app/users.py
---
