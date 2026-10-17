# Use cases

## Catch schema drift in CI

A column is renamed or retyped in a migration, and a query three modules away
still expects the old shape. With the schema dump in the repository,
`sqlty check` re-infers every query on each run and reports annotations that
no longer match: [`stale-type`](reference/rules.md#stale-type) for
`SQL[tuple[...]]`, and count, type and name mismatches for
[row types](guide/row-types.md).

Start with [Schema inference](guide/schema-inference.md), then
[CI](recipes/ci.md) to run the check on every pull request.

## Raw SQL without an ORM

You write SQL by hand for control or performance, and want typed results
without a query builder. sqlty keeps the SQL as a string, so it runs as
written, and adds the row type around it. With
[psycopg](guide/psycopg.md), `execute(conn, SQL[User](...))` returns a
`Cursor[User]`.

Start with [Writing queries](guide/writing-queries.md).

## Incremental adoption

An existing codebase has hundreds of untyped queries. sqlty only touches
strings that start with `--sql`, so you can convert one module at a time.
`sqlty check` lists prefixed strings not yet wrapped, and
`untyped = "sentinel"` marks the queries that still need casts or a schema.

Start with [Migration](recipes/migration.md).

## Scripts and notebooks

For a one-off script or a module of notebook helpers, the
[magic codec](guide/magic-codec.md) wraps `--sql` strings when the file is
imported, with no rewrite step. `SQL[T]` and `row_form` work the same way in
a notebook cell; the codec itself applies only to files that declare
`# coding: sqlty`.

## Guardrails for AI-generated SQL

Code assistants write plausible SQL whose result does not match the code
that consumes it. Make them use the `--sql` prefix, and run `sqlty rewrite`
and `sqlty check` as a pre-commit hook or CI step. A query whose columns do
not match the declared row type, or a `SELECT *` without a schema, fails
before review; [`rewrite --check`](recipes/ci.md#fail-on-unwrapped-queries) also
fails on queries left unwrapped. An agent gets the same feedback as a
person: one `file:line: [rule] message` line per problem and a non-zero exit
code (see the [CLI reference](reference/cli.md) and
[rules](reference/rules.md)).

Start with [CI](recipes/ci.md), and give your agent the
[sqlty skill](recipes/agents.md) so it writes queries this way from the start.
