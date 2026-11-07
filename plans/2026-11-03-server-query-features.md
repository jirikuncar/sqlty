# Server features in query strings

> **Status:** stage 1 (hover) merged in [#852](https://github.com/jirikuncar/sqlty/pull/852); stage 3 (join completion)
> merged in [#853](https://github.com/jirikuncar/sqlty/pull/853). Stage 4a (`sqlty format` with sqruff) merged in
> [#930](https://github.com/jirikuncar/sqlty/pull/930), stage 4b (the server's formatting) in
> [#969](https://github.com/jirikuncar/sqlty/pull/969); stage 4c (sqruff's lint rules in `sqlty check`) in
> [#972](https://github.com/jirikuncar/sqlty/pull/972). Stage 2 needs a decision on where signatures come from; stage 5
> waits for demand after stage 1.

Tracks [#847](https://github.com/jirikuncar/sqlty/issues/847): what `sqlty server` offers inside
a query string beyond diagnostics, quick fixes, completion and semantic tokens. Every stage needs
only the schema and the parsed query: no connection, no execution.

## Stages

| Stage | Content | State |
|-------|---------|-------|
| 1 | Hover: a column's type and source, a table's columns, a result column with its inference rule; `core.hover_source`, `# hover:` mdtests, `textDocument/hover`, `hover.enable` | merged, [#852](https://github.com/jirikuncar/sqlty/pull/852) |
| 2 | Signature help for functions the inference rules type, from their `function`, `argument` and `reference` | needs a decision: 36 of 389 function rules give arguments, none give arity or parameter names |
| 3 | Join completion: after `JOIN`, tables linked by a foreign key first, with the `ON` clause | merged, [#853](https://github.com/jirikuncar/sqlty/pull/853) |
| 4a | `sqlty format` and `core.format_source`: query strings laid out by sqruff's layout rules | merged, [#930](https://github.com/jirikuncar/sqlty/pull/930) |
| 4b | Formatting in `sqlty server`: the code action `source.format.sqlty` | merged, [#969](https://github.com/jirikuncar/sqlty/pull/969) |
| 4c | sqruff's lint rules in `sqlty check`: the `sqruff` rule | in review |
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

Decision on [#847](https://github.com/jirikuncar/sqlty/issues/847): format (and lint) the
detected query strings with sqruff, which neither `sqlfluff` nor `sqruff` does inside a
Python string on its own.

- `rust/crates/sqlty-format` embeds `sqruff-lib` with its layout rules and the raw
  templater, and reads no sqruff configuration file. It is its own crate so the WebAssembly
  engine and the no-I/O crates do not link it.
- A query is formatted when it is one plain triple-quoted literal without a backslash, so
  its text is the source text. The marker line, the first line's indentation and the
  whitespace before the closing quotes are kept. Placeholders (`%s`, `%(name)s`, `%%`, `?`)
  are swapped for names sqruff reads, then restored. A result that changes more than
  whitespace is dropped.
- `format` changes string content, unlike `rewrite` and `strip`; `# formatted:` mdtests
  check it is idempotent.
- Not covered in 4a: single-quoted strings (they would need `\n` escapes), concatenated
  literals, `sql.SQL` and helper arguments, which `Queries` finds only with the Python
  helper index.
- 4b: `sqlty server` offers `sqlty format` as the source action `source.format.sqlty`, run on
  the check worker like `source.fixAll.sqlty`. Not `textDocument/formatting`: VS Code runs one
  document formatter per language, so it would replace Ruff or Black instead of running
  beside them. `.sql` query files are left to sqruff itself.
- The playground's **Format** button is a `format` op of `sqlty-wasm`, asked on click rather
  than in `analyze`, which runs on every keystroke. Linking sqruff takes the engine from
  1.6 MB to 2.7 MB gzipped.
- 4c: the `sqruff` rule, off by default, reports what sqruff's core rules, less layout, find in
  every checked query, escaped strings included: `Queries::at` decodes the string and maps a
  finding back to the source. Placeholders become names of the same length, so offsets hold,
  and a finding on one is dropped. One rule for every sqruff code, with the code in the
  message; choosing codes would need a config key. Not in the playground's `analyze`, which
  runs on every keystroke. A linter is built once per thread and dialect: building sqruff's
  rules takes about a second, a query a few milliseconds after that.
