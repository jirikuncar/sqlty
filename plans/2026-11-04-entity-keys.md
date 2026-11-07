# Opt-in entity key types from foreign keys

> **Status:** stage 1 (`entity-mismatch`) merged in [#905](https://github.com/jirikuncar/sqlty/pull/905).
> Stage 2 (typed ids) needs a decision on where the generated `NewType` module lives and what
> it is called.

Tracks [#898](https://github.com/jirikuncar/sqlty/issues/898): keys of different tables share a
SQL type, so a join on the wrong key type-checks and returns plausible rows. Both stages are off
unless enabled.

## Stages

| Stage | What | Where |
|-------|------|-------|
| 1 | Rule `entity-mismatch` (default `ignore`): a comparison between columns holding keys of different tables | `Catalog::entity`, `analyzer/entities.rs` |
| 2 | `entity-types = true`: `rewrite` and `check --fix` write entity columns as `NewType`s from a generated module | `sqlty-python`, `_type_extractor` |

## Stage 1

- A primary or unique key column holds keys of its table; a foreign key column holds keys of
  the column it references, followed to the end of the chain (a cycle has no root). Composite
  keys match column by column: each column of a composite foreign key follows its own
  referenced column.
- Read in `ON`, `USING`, `WHERE` and `HAVING`: `=`, `<>`, `IN` lists, and `IN` subqueries
  that select one column.
- Gaps: views, CTEs and derived tables carry no entities (the issue's second open question).

## Stage 2

Open questions from the issue, to settle before building it:

- Where the generated module lives and what it is called. `rewrite` only edits strings in
  place today; writing a new Python file is a first.
- The `NewType` names: `CustomerId` from `customers` needs singularization, which is
  language-dependent. A config table mapping tables to names would avoid guessing.

Also needed, whatever the answers:

- A hand-written `NewType` must be accepted where its supertype is inferred, with the option
  off too. Class-based row types already erase it (`_type_extractor/subtype.py`), but
  `SQL[tuple[OrderId, CustomerId]]` is compared as text in `sqlty-python/src/checker.rs` and
  reported as `narrow-type`. The checker needs the module's `NewType`s to read the tuple.
