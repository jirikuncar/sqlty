---
type: regex
pattern: '\[tool\.sqlty\][^\[]*\bschema\s*=\s*"db/migrations/?"'
target:
  source: file
  path: pyproject.toml
---
