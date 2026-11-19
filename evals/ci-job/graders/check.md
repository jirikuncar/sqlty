---
type: regex
pattern: 'sqlty\s+check\b'
weight: 1
target:
  source: file
  path: .github/workflows/sql.yml
---
