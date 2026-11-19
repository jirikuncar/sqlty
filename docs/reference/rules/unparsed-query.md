# unparsed-query

Default severity: **warn**

## What it does

Reports a query that does not parse in the configured dialect: a `--sql`
string, the argument of `SQL[T](...)`, or a string passed to an
`Annotated[LiteralString, SQL]` parameter. The message quotes sqlparser's
error, which points at the token it could not read. A query whose expressions
nest more than 1000 levels deep, such as a chain of a thousand `OR`s, is not
analyzed either: its depth bounds the time and memory the analysis takes.

```toml
[tool.sqlty.rules]
unparsed-query = "warn"
```

```python
from sqlty import SQL

# error: [unparsed-query] "found: DOCUMENT"
q = SQL[tuple[str]]("--sql\nSELECT NULL::xml IS DOCUMENT")
```

## Why is this bad?

A query sqlty cannot parse gets no row type, so its annotation is not checked
against the columns and a rewrite skips it. Once a query is unparsed, the
coverage gap is silent unless this rule is on. `sqlty check --statistics`
counts such queries as `unparsed` either way.

## How to fix

Rewrite the query in a form the parser reads, or leave it: a query sqlparser
does not support in your dialect stays unchecked. To accept one case, put
`# sqlty: ignore[unparsed-query]` on its line.

## Configuration

A warning by default: sqlty's parser may not read SQL your database accepts, so the
warning lists the queries sqlty cannot check without failing CI. Make it fail, or turn
it off:

```toml
[tool.sqlty.rules]
unparsed-query = "ignore"
```
