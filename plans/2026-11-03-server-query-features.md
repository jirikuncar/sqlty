# Server features in query strings

> **Status:** stage 1 (hover) merged in [#852](https://github.com/jirikuncar/sqlty/pull/852); stage 3 (join completion)
> merged in [#853](https://github.com/jirikuncar/sqlty/pull/853). Stage 2 needs a decision on where signatures come
> from, stage 4 on whether to format at all; stage 5 waits for demand after stage 1.

Tracks [#847](https://github.com/jirikuncar/sqlty/issues/847): what `sqlty server` offers inside
a query string beyond diagnostics, quick fixes, completion and semantic tokens. Every stage needs
only the schema and the parsed query: no connection, no execution.

## Stages

| Stage | Content | State |
|-------|---------|-------|
| 1 | Hover: a column's type and source, a table's columns, a result column with its inference rule; `core.hover_source`, `# hover:` mdtests, `textDocument/hover`, `hover.enable` | merged, [#852](https://github.com/jirikuncar/sqlty/pull/852) |
| 2 | Signature help for functions the inference rules type, from their `function`, `argument` and `reference` | needs a decision: 36 of 389 function rules give arguments, none give arity or parameter names |
| 3 | Join completion: after `JOIN`, tables linked by a foreign key first, with the `ON` clause | merged, [#853](https://github.com/jirikuncar/sqlty/pull/853) |
| 4 | Formatting: decide whether sqlty formats at all | not started |
| 5 | Inlay hints of column types for `SELECT *` | waits for demand after stage 1 |

## Stage 1

- `sqlty_sql::analyzer::hover` reads the word under the cursor with completion's lexer.
  A word at the top level of a select item of the outermost `SELECT` is that result
  column: `analyze_select` gives its types, `explain_select` its rules. Any other name is
  what `complete` offers at the end of the word, so hover costs what completion does and
  resolves aliases, CTEs and subqueries the same way. A word before `.` lists the columns
  of that relation.
- A `*` item before the hovered one makes the index of its result column unknown, so it
  falls back to the name. DML statements (`INSERT ... SELECT`, `RETURNING`) get names only.
- `locate::hover` maps the word back to the source; the server answers from the buffer's
  last check, as completion does, never behind a check.
- VS Code shows the hovers of every provider together, so the extension needs no switch;
  `initializationOptions.hover.enable = false` turns it off for other clients.
- Not covered: functions and operators (stage 2 adds their signatures), parameters, and
  names in `INSERT` column lists.

## Stage 3

- The catalog keeps each table's enforced foreign keys, child to parent. `load` adds the
  reverse to the name index: `(parent, child, key)` sorted by parent and child name, so the
  children of a table whose name starts with what was typed are a binary search and a walk.
  `Catalog::joins_with_prefix` costs O(K + log F + k) for K keys of the table and F of the
  schema, never a function of the number of tables.
- After `JOIN` (not `JOIN schema.`), each `FROM` and `JOIN` item before the cursor offers
  the tables its keys link, either way, as a `join` candidate labelled
  `orders ON orders.user_id = u.id`, ranked before the plain names. Both sides are written
  as the query writes them: the alias in scope, and the table's bare name, or
  `schema.name` when a bare name finds another table.
- A table whose name is already a qualifier in scope is left out: its `ON` clause would be
  ambiguous without an alias, and sqlty does not invent one.
- Not covered: deferrable, `NOT ENFORCED` and `NOT VALID` keys, which the catalog drops;
  views and CTEs, which have no keys.

## Stage 4

Neither `sqlfluff` nor `sqruff` formats a query inside a Python string. A formatter here
would have to keep `strip(rewrite(x)) == x` and every escape sequence of the string, and
would reformat code the user wrote by hand. Decide with the maintainer before building it.
