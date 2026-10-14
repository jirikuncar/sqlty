# unused-ignore

Default severity: **ignore**

## What it does

Reports a `# sqlty: ignore` comment that suppresses no diagnostic. A bare
comment is reported when nothing on its line is suppressed; a comment with a
rule list is reported once per listed rule that suppresses nothing.

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[select-star]
```

`sqlty check` reports `'# sqlty: ignore[select-star]' suppresses no diagnostic`
when this rule is enabled.

## Why is this bad?

A suppression outlives the problem it hid. Once the query or the row type is
fixed, a stale comment would silently accept a future regression on that line.

## How to fix

Remove the comment, or correct the rule name if it is misspelled.

## Configuration

Off by default. Enable it to keep suppressions honest:

```toml
[tool.sqlty.rules]
unused-ignore = "warn"
```
