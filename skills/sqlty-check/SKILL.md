---
name: sqlty-check
description: Run sqlty check and act on its diagnostics, name the row type of a query, and check query parameters, including the typed parameters of SQL[R, P]. Use when sqlty check reports a diagnostic such as [unknown-column] or [narrow-type], when replacing SQL[tuple[...]] with a TypedDict, dataclass, NamedTuple or pydantic model, when a column may be NULL, when a call passes the wrong parameters, when a join inflates a SUM, AVG or COUNT ([fanout-aggregate], [chasm-join]) or compares keys of different tables ([entity-mismatch]), when suppressing a diagnostic with a sqlty ignore comment, or when changing a rule's severity in [tool.sqlty.rules].
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
| `sqlty check --output-format json .` | Read the diagnostics as data: `file`, `line`, `rule`, `severity`, `message`, the `start` and `end` they underline, and the `fix` edits |
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
row type: update its fields yourself. For `unknown-column` it writes the closest
column of the tables in scope, when there is one: read the suggestion before you
accept it.

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

### Typed parameters

`SQL[R, P]` types the parameters of a query as `P`: a `tuple` with one item per
placeholder, or a `TypedDict` with one key per name. `sqlty check` compares a `P`
you wrote with the query (`params-type-mismatch`). `sqlty rewrite` writes it;
a type checker is then stricter than the driver, and `param-types = false` turns
it off.

```python
from sqlty import SQL

# error: [params-type-mismatch] "tuple[int, int] has 2 items but the query has 1 parameter"
q = SQL[tuple[int], tuple[int, int]]("--sql\nSELECT 1 FROM users LIMIT %s")
```

### Queries built from parts

A query that isn't a string literal is skipped as `dynamic-query`, which is off
by default. With `preview = true`, a query built from literals bound in branches is
checked along every path (`max-query-variants`, `64` by default, caps the paths; a
query with more is reported, never checked in part). See
[Query variants](https://sqlty.kuncar.dev/guide/query-variants/).

```toml
[tool.sqlty]
preview = true
```

`type-modules` is gone: `check` reads row types from source and never imports a
module, so don't add it. A name it can't read is unknown and never reported.

## Fix joins that inflate aggregates

`fanout-aggregate` and `chasm-join` (warnings) report a `SUM`, `AVG` or
`COUNT` over rows a one-to-many join repeats; sqlty reads the primary, unique
and foreign keys of the schema. Don't add `DISTINCT` to silence it: two orders
with the same total would collapse. Aggregate the many side at its own grain
first, then join one row per key:

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, total NUMERIC NOT NULL);
CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders, amount NUMERIC NOT NULL);
```

```python
from decimal import Decimal
from sqlty import SQL

# error: [fanout-aggregate] "SUM(o.total) aggregates orders, whose rows repeat once per order_items row"
bad = SQL[tuple[Decimal | None, Decimal | None]]("--sql\nSELECT SUM(o.total), SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id")

# error:
good = SQL[tuple[Decimal | None, Decimal | None]]("""--sql
WITH items AS (SELECT order_id, SUM(amount) AS amount FROM order_items GROUP BY order_id)
SELECT SUM(o.total), SUM(i.amount) FROM orders o JOIN items i ON i.order_id = o.id""")
```

A key the data has but the schema doesn't declare is unknown to sqlty: declare
it (`UNIQUE`, a unique index) instead of suppressing. `unproven-join` (off by
default) lists joins no key covers.

`entity-mismatch` (off by default; enable with `entity-mismatch = "warn"`)
reports a comparison such as `products.id = orders.customer_id`. Join on the
key the foreign key references; suppress only a polymorphic id that is
compared to several tables on purpose.

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

- [Rules](https://sqlty.kuncar.dev/reference/rules/), and `sqlty rule NAME`
- [Row types](https://sqlty.kuncar.dev/guide/row-types/)
- [Nullability](https://sqlty.kuncar.dev/guide/schema-inference/#nullability)
- [Parameters](https://sqlty.kuncar.dev/guide/parameters/)
- [Typed parameters](https://sqlty.kuncar.dev/guide/parameters/#typed-parameters)
- [Query variants](https://sqlty.kuncar.dev/guide/query-variants/)
- [Joins and aggregates](https://sqlty.kuncar.dev/guide/joins/)
- [Python API](https://sqlty.kuncar.dev/reference/python-api/)
