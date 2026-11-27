---
description: Type-check the SQL of TypeScript and JavaScript tagged templates against your schema in ESLint, with no database, using @sqlty/eslint-plugin.
---

# TypeScript and JavaScript (ESLint)

[`@sqlty/eslint-plugin`](https://www.npmjs.com/package/@sqlty/eslint-plugin) runs the
sqlty engine on the `sql` tagged templates of TypeScript and JavaScript, as ESLint
lints them. It reads your schema from `.sql` files, so it needs no database: not in
CI, not in a pre-commit hook, not in the editor.

```console
npm install --save-dev eslint @typescript-eslint/parser @sqlty/eslint-plugin
```

## What it solves

TypeScript sees a query as a string. A misspelled column, a dropped table or a row type
that no longer matches the schema compiles, passes review, and fails at runtime, or
returns `undefined` where the code expects a value.

- **Mistakes in SQL** are reported where the template writes them, with the fix, as you
  type and in CI, before the query runs.
- **Row types** come from the schema. `eslint --fix` writes them, and a migration that
  changes a column flags every query whose row type it breaks.
- **Driver surprises**: `bigint` and `numeric` arrive as strings, an enum array as text
  in node-postgres. The row type says what the driver hands you, not the SQL type.
- **No database**: the schema is read from `.sql` files, so the check runs in a
  pre-commit hook, a fork's CI or an editor with no PostgreSQL to connect to.

For coding agents, the same check is the feedback loop for the SQL they write. An agent's
sandbox rarely has the database, yet `npx eslint .` names the wrong column, the line and
the fix, `npx eslint . --fix` writes the row types, and `--format json` reads as data.
The answer depends only on the schema and the code, so a retry gets the same one.

## Configure

```js
// eslint.config.js
import parser from "@typescript-eslint/parser";
import sqlty from "@sqlty/eslint-plugin";

const check = ["error", { schema: "schema.sql", driver: "postgres", targets: [{ tag: "sql", transform: "{type}[]" }] }];

export default [
  { files: ["**/*.js"], plugins: { sqlty }, rules: { "sqlty/check-sql": check } },
  { files: ["**/*.ts"], languageOptions: { parser }, plugins: { sqlty }, rules: { "sqlty/check-sql": check } },
];
```

## What it reports

A typo is reported where the template writes it, with the fix as a suggestion:

```ts
const sql = postgres();

export const recent = () => sql`SELECT id, emial FROM users ORDER BY id DESC`;
//                                         ^^^^^ column `emial` does not exist in `users`; did you mean `email`?
```

In TypeScript, a query without its row type, or with one that no longer matches the
query, gets it from `eslint --fix`:

```ts
export const user = (id: number) =>
  sql<{ id: number; name: string; email: string | null }[]>`SELECT id, name, email FROM users WHERE id = ${id}`;
```

Each interpolation is a parameter (`$1`, `$2`, ...). JavaScript has no type
arguments, so a `.js` file gets the SQL problems only.

## Options

| Option | |
|--------|-|
| `schema` | a `.sql` file, or a list of them, with the `CREATE TABLE` statements, relative to the working directory |
| `migrationsDir` | a directory whose `.sql` files, in name order, make the schema |
| `dialect` | `postgres` (default) |
| `driver` | `postgres` ([postgres.js](https://github.com/porsager/postgres), default) or `pg` ([node-postgres](https://node-postgres.com/)) |
| `targets` | `[{ tag, transform, fieldTransform, skipTypeAnnotations }]`: `tag` is a name such as `sql` or `db.query`, or a `/regex/` |
| `overrides.types` | the TypeScript type of a SQL type, such as `{ "BIGINT": "bigint" }` |
| `rules` | sqlty rule severities, such as `{ "unknown-column": "warn" }` |
| `enforceType` | `fix` (default) or `suggest` |

The options follow [SafeQL](https://safeql.dev/api/)'s, which asks a running
PostgreSQL to type the same templates.

## Drivers

The row type is what the driver hands JavaScript, which is not always the SQL type:

| SQL type | postgres.js 3.4 | node-postgres 8 |
|----------|-----------------|-----------------|
| `int`, `smallint`, `real`, `double precision` | `number` | `number` |
| `bigint`, `numeric` | `string` | `string` |
| `date`, `timestamp`, `timestamptz` | `Date` | `Date` |
| `interval` | `string` | `IPostgresInterval` |
| `numeric[]` | `string[]` | `number[]` |
| an array of an enum | `string[]` | `string`, the array's literal text |
| `json`, `jsonb` | `unknown` | `unknown` |
| `bytea` | `Buffer` | `Buffer` |

`overrides.types` changes one, such as `bigint` for a postgres.js client created with
`types: { bigint: postgres.BigInt }`.

## Limits

- PostgreSQL only for now.
- A template that interpolates another `sql` fragment or call is skipped.
- The types of interpolated values are not checked against their parameters.
