# param-count-mismatch

Default severity: **error**

## What it does

Compares the arguments a call passes with the placeholders of its query. It
applies to calls of a [marked helper](../../../docs/guide/helpers.md) whose parameters are
spelled out (a `params` argument holding a tuple, list or dict literal,
`*args`, or `**params`), and reports:

- a different number of positional arguments than the query has placeholders
  (`%s`, `$1`, `?`; the highest `$N` counts);
- named arguments that the query does not name (`%(name)s`, `:name`), or
  names the call does not pass. A name used twice is one parameter.

Positional arguments for named placeholders, or the reverse, and calls that do
not spell their arguments out (`params` as a variable, `*rest`, `**kwargs`),
are not compared.

## Why is this bad?

The driver raises at run time, or the query silently runs with the wrong
values, and only on the path that runs it.

## Example

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...] | dict[str, object]) -> None: ...


# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
execute("SELECT id FROM users WHERE id = %s AND name = %s", (1,))
# error: [param-count-mismatch] "query and call parameters differ: missing 'name'"
execute("SELECT id FROM users WHERE id = %(id)s AND name = %(name)s", {"id": 1})
```

## Fix

Pass one argument per placeholder, or name each placeholder in the call.
Turn the rule off with `param-count-mismatch = "ignore"` in
`[tool.sqlty.rules]`.
