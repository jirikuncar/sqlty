# tstring-skipped

Default severity: **warn**

## What it does

Flags t-strings ([PEP 750](https://peps.python.org/pep-0750/)) starting with
`--sql` that rewrite leaves alone, and names the field that blocks them. A
field is read when it passes a value:

| Field | Read as |
|-------|---------|
| `{value}`, `{value:s}`, `{value:b}`, `{value:t}` | a placeholder |
| `{value:l}` (psycopg literal) | a placeholder |
| `{name:i}` (psycopg identifier) | an identifier, only in a query that returns no rows |
| `{part:q}` (psycopg snippet) | the module-level template `part` names, inlined |

`:l`, `:i` and `:q` are psycopg's, so they are read only in a module that
imports `psycopg` or `sqlty.drivers.psycopg`. Any other field, such as a
conversion (`{value!r}`) or another format spec, is skipped. A t-string whose
fields are all read is rewritten as `SQLTemplate[T](t"...")` and checked; see
[t-strings](../../../docs/guide/writing-queries.md#t-strings).

## Why is this bad?

Such a field changes the text of the SQL in a way only known at runtime, so the
query can't be analyzed statically.

## Example

An identifier picks the result column:

```python
import psycopg

column = "id"
# error: [tstring-skipped] "t-string with --sql prefix skipped (the field {column:i} changes the SQL)"
q = t"--sql\nSELECT {column:i}::INTEGER FROM users"
```

Outside a psycopg module, `:i`, `:l` and `:q` are not read:

```python
table = "users"
name = "Ada"
# error: [tstring-skipped] "t-string with --sql prefix skipped (the field {table:i} changes the SQL)"
q = t"--sql\nINSERT INTO {table:i} (name) VALUES ({name})"
```

A snippet must name a template assigned once at module level:

```python
import psycopg

def condition():
    return t"id = 1"

# error: [tstring-skipped] "t-string with --sql prefix skipped (the field {condition():q} changes the SQL)"
q = t"--sql\nSELECT id::INTEGER FROM users WHERE {condition():q}"
```

A conversion is never read:

```python
user_id = 1
# error: [tstring-skipped] "t-string with --sql prefix skipped (the field {user_id!r} changes the SQL)"
q = t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id!r}"
```
