---
type: regex
pattern: 'RENAME\s+COLUMN|DROP\s+COLUMN'
flags: i
match: not_contains
target:
  source: file
  path: db/migrations/0002_full_name.sql
---
