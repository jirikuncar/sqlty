---
type: regex
pattern: 'sqlty\s+breaking\b[^\n]*--against'
weight: 1
target:
  source: file
  path: .github/workflows/sql.yml
---
