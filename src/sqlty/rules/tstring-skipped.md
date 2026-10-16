# tstring-skipped

Default severity: **warn**

## What it does

Flags t-strings ([PEP 750](https://peps.python.org/pep-0750/)) starting with
`--sql` that rewrite leaves alone: those with a field that has a format spec or
a conversion, such as psycopg's `{name:i}` (identifier), `{value:l}` (literal)
or `{part:q}` (a nested query). A t-string whose fields are plain values is
rewritten as `SQLTemplate[T](t"...")` and checked; see
[t-strings](../../../docs/guide/writing-queries.md#t-strings).

## Why is this bad?

Such a field changes the text of the SQL, which is only known at runtime, so
the query can't be analyzed statically.

## Example

```python
# error: [tstring-skipped] "t-string with --sql prefix skipped (a field with a format spec or conversion changes the SQL)"
q = t"--sql\nSELECT {column:i}::INTEGER FROM users"
```
