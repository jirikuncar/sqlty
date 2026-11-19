---
description: Reference for every sqlty check rule, its default severity and what it reports, and how to change severities in [tool.sqlty.rules].
---

# Rules

`sqlty check` reports diagnostics as `file:line: [rule] message`. Each rule
has a default severity, changed per rule in `[tool.sqlty.rules]`:

```toml
[tool.sqlty.rules]
column-name-mismatch = "error"
stale-type = "error"
```

`error` makes `sqlty check` exit with 1, `warn` prints without failing, and
`ignore` turns the rule off. For one run, `--error RULE`, `--warn RULE` and
`--ignore RULE` override the table; see [CLI](cli.md#configuration-flags).

<!-- generated:rules -->
| Rule | Group | Default | Fixable | Reports |
|------|-------|---------|---------|---------|
| [`column-count-mismatch`](rules/column-count-mismatch.md) | Row type | error | no | Row type has a different number of fields than the query has columns |
| [`column-type-mismatch`](rules/column-type-mismatch.md) | Row type | error | no | Row type field can't hold its column's values |
| [`column-name-mismatch`](rules/column-name-mismatch.md) | Row type | warn | no | NamedTuple field name differs from its column name |
| [`typeddict-key-mismatch`](rules/typeddict-key-mismatch.md) | Row type | error | no | Columns don't fit the fields of a TypedDict, dataclass or Pydantic model |
| [`typeddict-inheritance`](rules/typeddict-inheritance.md) | Row type | error | no | TypedDict subclass breaks a closed or extra_items base (PEP 728) |
| [`nullable-mismatch`](rules/nullable-mismatch.md) | Row type | error | no | Row type field is not optional but its column can be NULL |
| [`json-shape-mismatch`](rules/json-shape-mismatch.md) | Row type | error | no | Row type field can't hold the JSON its column builds |
| [`redundant-optional`](rules/redundant-optional.md) | Row type | ignore | no | Row type field is optional but its column cannot be NULL |
| [`literal-mismatch`](rules/literal-mismatch.md) | Row type | error | no | Row type field is a Literal that does not cover its column's values |
| [`redundant-literal`](rules/redundant-literal.md) | Row type | ignore | no | Row type field is a Literal that lists values its column never takes |
| [`select-star`](rules/select-star.md) | Query | warn | yes | SELECT * that sqlty check --fix expands to a column list |
| [`select-star-no-schema`](rules/select-star-no-schema.md) | Query | error | no | SELECT * with no schema to expand it |
| [`mysql-sql-marker`](rules/mysql-sql-marker.md) | Query | warn | yes | --sql marker with dialect = "mysql", which MySQL rejects; use "-- sql" |
| [`inline-sql-comment`](rules/inline-sql-comment.md) | Query | warn | yes | "--sql SELECT ..." is a SQL comment; use "--sql\nSELECT ..." or run sqlty check --fix |
| [`unparsed-query`](rules/unparsed-query.md) | Query | warn | no | Query that does not parse, so sqlty cannot check it |
| [`sqruff`](rules/sqruff.md) | Query | ignore | no | Finding of sqruff's core lint rules in a query |
| [`unknown-database`](rules/unknown-database.md) | Query | error | no | Query marker names a database the project does not configure |
| [`unknown-table`](rules/unknown-table.md) | Names | error | no | Table the schema does not define |
| [`unknown-column`](rules/unknown-column.md) | Names | error | yes | Column the table does not have |
| [`ambiguous-column`](rules/ambiguous-column.md) | Names | error | no | Unqualified column that more than one table of the query has |
| [`duplicate-alias`](rules/duplicate-alias.md) | Names | error | no | Two tables of a FROM clause read by the same name |
| [`unknown-function`](rules/unknown-function.md) | Names | warn | no | Function whose result sqlty cannot type |
| [`unknown-type`](rules/unknown-type.md) | Names | warn | no | Cast or column type without a Python type |
| [`unsupported-syntax`](rules/unsupported-syntax.md) | Names | warn | no | Construct that parses but is not typed |
| [`insert-missing-column`](rules/insert-missing-column.md) | Names | error | no | INSERT that leaves out a NOT NULL column without a default |
| [`insert-value-count`](rules/insert-value-count.md) | Names | error | no | INSERT that gives a different number of values than the columns it writes |
| [`ungrouped-column`](rules/ungrouped-column.md) | Names | error | no | Grouped query selects a column it neither groups by nor aggregates |
| [`fanout-aggregate`](rules/fanout-aggregate.md) | Names | warn | no | Aggregate over rows a one-to-many join repeats |
| [`chasm-join`](rules/chasm-join.md) | Names | warn | no | Aggregating query joins one table one-to-many to two others |
| [`unproven-join`](rules/unproven-join.md) | Names | ignore | no | Join in an aggregating query that matches no primary or unique key |
| [`entity-mismatch`](rules/entity-mismatch.md) | Names | ignore | no | Comparison between columns holding keys of different tables |
| [`rejected-query`](rules/rejected-query.md) | Names | error | no | Query the database rejects for a reason no other rule names |
| [`stale-rewrite`](rules/stale-rewrite.md) | Rewrite | warn | yes | --sql string not wrapped in SQL[T] |
| [`stale-type`](rules/stale-type.md) | Rewrite | warn | yes | SQL[tuple[...]] wider than the inferred row type |
| [`narrow-type`](rules/narrow-type.md) | Rewrite | error | yes | SQL[tuple[...]] doesn't admit every row the query returns |
| [`fstring-skipped`](rules/fstring-skipped.md) | Rewrite | warn | no | f-string with --sql cannot be rewritten |
| [`tstring-skipped`](rules/tstring-skipped.md) | Rewrite | warn | no | t-string with --sql and a field that changes the SQL |
| [`unchecked-query`](rules/unchecked-query.md) | Rewrite | warn | no | --sql query that is not plain string literals |
| [`dynamic-query`](rules/dynamic-query.md) | Rewrite | ignore | no | Helper query that is not a string literal |
| [`untyped-query`](rules/untyped-query.md) | Rewrite | warn | yes | SQL[UNTYPED] query |
| [`param-count-mismatch`](rules/param-count-mismatch.md) | Params | error | no | Call passes a different number of parameters than its query has |
| [`extra-param`](rules/extra-param.md) | Params | warn | no | Call passes a key its query has no placeholder for |
| [`param-type-mismatch`](rules/param-type-mismatch.md) | Params | error | no | Call passes an argument the query's parameter can't take |
| [`params-type-mismatch`](rules/params-type-mismatch.md) | Params | error | no | P of SQL[R, P] doesn't fit the query's parameters |
| [`unused-ignore`](rules/unused-ignore.md) | Suppression | warn | no | # sqlty: ignore comment that suppresses nothing |
| [`internal-error`](rules/internal-error.md) | Internal | error | no | sqlty crashed checking the file |
<!-- /generated:rules -->

`nullable-mismatch` and `redundant-optional` check nullability; see
[Nullability](../guide/schema-inference.md#nullability). `literal-mismatch` and
`redundant-literal` check `Literal` annotations against the values a column can
take; see [Literal values](../guide/schema-inference.md#literal-values).

`sqlty check --fix` applies the fixable rules; `sqlty rule NAME` prints the
rule's page, the same page the table links to.

## Inline suppression

A comment on the line a diagnostic is reported on drops it:

- `# sqlty: ignore` drops every diagnostic on that line.
- `# sqlty: ignore[rule-a, rule-b]` drops only the listed rules. A name that
  matches no diagnostic on the line drops nothing, so the diagnostic stays.

The comment is read with Python's tokenizer, so a `#` inside a string does
not start one. For a statement that spans lines, put the comment on the line
the diagnostic names. `sqlty check --fix` keeps the comment and does not apply
a fix for a rule the comment suppresses.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error:
q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[column-type-mismatch]
```

Enable [`unused-ignore`](rules/unused-ignore.md) to report comments that suppress
nothing, so accepted cases do not outlive their reason.

## Rule groups

- **Row type** rules compare a [row type](../guide/row-types.md) (`TypedDict`,
  `NamedTuple`, dataclass, Pydantic model) defined in the same file with the
  query's columns.
- **Query** rules judge the query string itself: its marker, its `*` and
  whether it parses.
- **Name** rules explain why a result column is untyped and report SQL the
  database rejects. With the first six enabled and a schema configured, no
  column is left untyped without a diagnostic. `insert-missing-column` and
  `insert-value-count` check what an `INSERT` writes, and `ungrouped-column`
  what a grouped query selects. `fanout-aggregate`, `chasm-join` and
  `unproven-join` check what joins do to aggregates, and `entity-mismatch` what
  they compare; `rejected-query` reports the rest of what the database rejects.
  The rules for SQL the database rejects are errors; those for valid SQL sqlty
  cannot type are warnings. All but `unsupported-syntax`, `duplicate-alias` and
  `rejected-query` are only judged when a [schema](../guide/schema-inference.md)
  is configured, because without one every table, column, function and type is
  unknown. Tables, columns and ambiguity are checked only in the `postgres`,
  `mysql` and `sqlite` dialects; for any other, `sqlty check` prints a note when
  a schema is configured. The
  [name diagnostics guide](../guide/name-diagnostics.md) shows how to change them.
- **Rewrite** rules compare the source with what `sqlty rewrite` would write.
- **Params** rules read the arguments a [query helper](../guide/helpers.md) or
  driver call passes for the query's placeholders, and the `P` of `SQL[R, P]`;
  see [Query parameters](../guide/parameters.md).
