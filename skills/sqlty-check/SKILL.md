---
name: sqlty-check
description: Run sqlty check and act on its diagnostics, name the row type of a query, and check query parameters. Use when sqlty check reports a diagnostic such as [unknown-column] or [stale-type], when replacing SQL[tuple[...]] with a TypedDict, dataclass, NamedTuple or pydantic model, when a column may be NULL, when a call passes the wrong parameters, when suppressing a diagnostic with a sqlty ignore comment, or when changing a rule's severity in [tool.sqlty.rules].
---

# sqlty check

`sqlty check` compares each query with its row type, its parameters and the
schema, and prints one `file:line: [rule] message` per problem. It exits 1 when
a rule with severity `error` fires; warnings alone exit 0.

Run `sqlty version`: this skill may be newer or older than the installed sqlty.
When it and `sqlty --help` or `sqlty rule NAME` disagree, trust the command.

## Read a diagnostic

1. Run `sqlty rule NAME` with the rule in brackets: it prints what the rule
   reports, why, and how to fix it.
2. For a type you didn't expect, run `sqlty check --explain <file>`: under each
   type diagnostic it prints the inference rule behind each column's type and
   nullability, with a link to the vendor documentation.
3. Fix the query, the row type or the schema. Suppress only what sqlty gets
   wrong, and say why.

| Command | Use |
|---------|-----|
| `sqlty check .` | Lint everything |
| `sqlty check --fix .` | Apply the fixable rules, then report the rest |
| `sqlty check --output-format json .` | Read the diagnostics as data: `file`, `line`, `rule`, `severity`, `message` |
| `sqlty check --error-on-warning .` | Fail on warnings too |
| `sqlty check --warn unknown-column .` | Change one rule's severity for one run |
| `sqlty rule` | List every rule with its default severity |

## Name the row type

Replace `SQL[tuple[...]]` with a `TypedDict`, `dataclass`, `NamedTuple` or
pydantic model whose fields are the columns. `sqlty check` compares each
field's name, type and nullability with the query:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# error: [column-type-mismatch] "User.name is str but query column 'name' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::INTEGER AS name FROM users")
```

`sqlty check --fix` updates a stale `SQL[tuple[...]]` but never edits a named
row type: update its fields yourself.

### Nullable columns

A column that can be NULL needs `T | None`: a schema column without `NOT NULL`,
the optional side of an outer join, an aggregate over no rows.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# error: [nullable-mismatch] "User.name is str but query column 'name' can be NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

Annotate `name: str | None`, or filter or `COALESCE` the NULLs away.

## Check parameters

A call of a driver or a marked helper is checked when it spells out its
arguments. An argument of unknown type is never reported.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...] = (), /) -> None: ...

execute("SELECT id FROM users WHERE id = %s AND age > %s", (1, b"1"))

# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
execute("SELECT id FROM users WHERE id = %s AND age > %s", (1,))
```

## Suppress a diagnostic

`# sqlty: ignore[rule]` on the reported line drops that rule there. Name the
rule: a bare `# sqlty: ignore` drops everything on the line.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error:
q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[column-type-mismatch]
```

To change a rule for the whole project, set its severity (`error`, `warn` or
`ignore`) in `[tool.sqlty.rules]`:

```toml
[tool.sqlty.rules]
stale-type = "error"
```

## Inspect one query

The Python API types a query without a file:

```python
from sqlty.core import analyze_params, infer_type

schema = "CREATE TABLE users (id INTEGER NOT NULL, name TEXT)"
infer_type("SELECT id, name FROM users", schema_sql=schema)  # 'tuple[int, str | None]'
analyze_params("SELECT name FROM users WHERE id = %s", schema_sql=schema)  # %s is an int
```

## Reference

- [Rules](https://jirikuncar.github.io/sqlty/reference/rules/), and `sqlty rule NAME`
- [Row types](https://jirikuncar.github.io/sqlty/guide/row-types/)
- [Nullability](https://jirikuncar.github.io/sqlty/guide/schema-inference/#nullability)
- [Parameters](https://jirikuncar.github.io/sqlty/guide/parameters/)
- [Python API](https://jirikuncar.github.io/sqlty/reference/python-api/)
