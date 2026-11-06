# sqruff

Default severity: **ignore**

## What it does

Reports what the core lint rules of [sqruff](https://github.com/quarylabs/sqruff)
find in a query, in the project's dialect. The message starts with the sqruff
rule code, such as `AM06`, and the underline is the part of the query it is about.

```toml
[tool.sqlty.rules]
sqruff = "warn"
```

```python
from sqlty import SQL

# error: [sqruff] "AM06: Inconsistent column references in 'GROUP BY/ORDER BY' clauses."
query = SQL[tuple[str, int]]("""--sql
SELECT name, COUNT(*) AS n
FROM users
GROUP BY 1
ORDER BY name""")
```

Every query sqlty checks is linted, whatever its string looks like: escape
sequences are read, and the underline lands on the source they are written in.
Placeholders (`%s`, `%(name)s`, `?`) are read as names, and a finding on one is
dropped.

## Why is this bad?

sqruff's core rules catch queries that run but read badly or mean something
other than they seem: a `DISTINCT` that looks like a function, an alias never
used, an `ORDER BY` that mixes positions and names.

## How to fix

Follow the rule: the [sqruff rules](https://playground.quary.dev/docs/reference/rules/) page
explains each code.

## Configuration

Off by default, as these rules judge style rather than types. Turn it on per
project:

```toml
[tool.sqlty.rules]
sqruff = "warn"
```

sqruff's layout rules are left out: [`sqlty format`](../../../docs/guide/formatting.md)
lays out queries. sqruff reads no configuration file here, so the rules are the
same on every machine. `# sqlty: ignore[sqruff]` on the line a query's string
starts silences all its findings.
