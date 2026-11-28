# @sqlty/eslint-plugin

Type-checks the SQL of tagged templates in TypeScript against your schema, with no database:
unknown tables and columns where they are written, and the row type of each query written
or fixed as its type argument. The [sqlty](https://sqlty.kuncar.dev/) engine runs in
WebAssembly ([`@sqlty/wasm`](../wasm)).

```ts
const rows = await sql`SELECT id, name, email FROM users WHERE id = ${id}`;
// eslint --fix:
const rows = await sql<{ id: number; name: string; email: string | null }[]>`SELECT ...`;
```

TypeScript sees a query as a string, so a misspelled column or a row type the schema no
longer matches compiles and fails at runtime. This plugin catches both in the editor, a
pre-commit hook or CI, and gives coding agents a check that needs no database in their
sandbox: the wrong column, its line and the fix, with `--format json` for tools.

```js
// eslint.config.js
import parser from "@typescript-eslint/parser";
import sqlty from "@sqlty/eslint-plugin";

export default [
  {
    files: ["**/*.ts"],
    languageOptions: { parser },
    plugins: { sqlty },
    rules: {
      "sqlty/check-sql": ["error", { schema: "schema.sql", driver: "postgres", targets: [{ tag: "sql", transform: "{type}[]" }] }],
    },
  },
];
```

| Option | |
|--------|-|
| `schema` | a `.sql` file, or a list of them, with the `CREATE TABLE` statements, relative to the working directory |
| `migrationsDir` | a directory whose `.sql` files, in name order, make the schema |
| `dialect` | `postgres` (default) |
| `driver` | `postgres` ([postgres.js](https://github.com/porsager/postgres), default) or `pg` ([node-postgres](https://node-postgres.com/)): what JavaScript type each SQL type arrives as |
| `targets` | `[{ tag, transform, fieldTransform, skipTypeAnnotations }]`: `tag` is a name such as `sql` or `db.query`, or a `/regex/` |
| `overrides.types` | TypeScript type per SQL type, such as `{ "BIGINT": "bigint" }` |
| `rules` | sqlty rule severities, such as `{ "unknown-column": "warn" }` |
| `enforceType` | `fix` (default) or `suggest` |

Interpolations are parameters (`$1`, `$2`, ...). A template that interpolates another
`sql` fragment or call is skipped.

The drivers differ: `bigint` and `numeric` are strings in both, `interval` is an object in
node-postgres and a string in postgres.js, `numeric[]` is `number[]` in node-postgres and
`string[]` in postgres.js, and node-postgres leaves arrays of types it does not register (an
enum's) as their literal text.
