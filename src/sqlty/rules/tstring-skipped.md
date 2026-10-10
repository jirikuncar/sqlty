# tstring-skipped

Default severity: **warn**

## What it does

Flags t-strings ([PEP 750](https://peps.python.org/pep-0750/)) starting with
`--sql`, which rewrite leaves alone.

## Why is this bad?

Template strings are interpolated at runtime, so the query can't be analyzed
statically.

## Example

```python
# error: [tstring-skipped] "t-string with --sql prefix skipped (t-strings cannot be statically rewritten)"
q = t"--sql\nSELECT {column}::INTEGER FROM users"
```
