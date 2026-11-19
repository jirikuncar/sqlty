---
type: regex
pattern: 'ADD\s+COLUMN\s+(IF\s+NOT\s+EXISTS\s+)?full_name'
flags: i
target:
  source: file
  path: db/migrations/0002_full_name.sql
---
