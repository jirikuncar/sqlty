import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { RuleTester } from "eslint";
import parser from "@typescript-eslint/parser";
import plugin from "../index.js";

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.itOnly = it.only;

const schema = fileURLToPath(new URL("./fixtures/schema.sql", import.meta.url));
const empty = fileURLToPath(new URL("./fixtures/empty.sql", import.meta.url));
const tester = new RuleTester({ languageOptions: { parser } });
const rule = plugin.rules["check-sql"];
const options = (more = {}) => [{ schema, ...more }];
const user = "{ id: string; name: string; email: string | null }";

tester.run("check-sql", rule, {
  valid: [
    { options: options(), code: `sql<${user}>\`SELECT id, name, email FROM users\`` },
    { options: options(), code: "sql<{ n: number }>`SELECT 1 AS n`" },
    { options: options(), code: "sql`DELETE FROM users WHERE id = ${id}`" },
    { options: options(), code: "other`SELECT nope FROM nowhere`" },
    { options: options(), code: "sql`SELECT * FROM ${sql(table)}`" },
    {
      options: options({ targets: [{ tag: "sql", skipTypeAnnotations: true }] }),
      code: "sql`SELECT id FROM users`",
    },
    {
      options: options({ dialect: "mysql", targets: [{ tag: "sql", skipTypeAnnotations: true }] }),
      code: "sql`SELECT id FROM users WHERE id = ${id}`",
    },
    { options: [{ schema: empty }], code: "sql<{ n: number }>`SELECT 1 AS n`" },
    {
      options: options({ targets: [{ tag: "/db\\.(one|many)/" }] }),
      code: "db.one<{ id: string }>`SELECT id FROM users WHERE name = ${name}`",
    },
  ],
  invalid: [
    {
      name: "a JavaScript file gets SQL problems, not row types",
      filename: "queries.js",
      options: options(),
      code: "sql`SELECT nme FROM users`; sql`SELECT id FROM users`",
      errors: [{ messageId: "sql", suggestions: 1 }],
    },
    {
      name: "a missing row type is added after the tag",
      options: options(),
      code: "const rows = await sql`SELECT id, name, email FROM users WHERE id = ${id}`",
      output: `const rows = await sql<${user}>\`SELECT id, name, email FROM users WHERE id = \${id}\``,
      errors: [{ messageId: "missingTypeAnnotation" }],
    },
    {
      name: "a wrong row type is replaced",
      options: options(),
      code: "sql<{ id: number }>`SELECT id FROM users`",
      output: "sql<{ id: string }>`SELECT id FROM users`",
      errors: [{ messageId: "incorrectTypeAnnotation", line: 1, column: 4 }],
    },
    {
      name: "an unknown column is reported where it is written, with a suggestion",
      options: options(),
      code: "sql`SELECT nme FROM users WHERE id = ${id}`",
      errors: [
        {
          message: "column `nme` does not exist in `users`; did you mean `name`? [unknown-column]",
          column: 12,
          endColumn: 15,
          suggestions: [{ messageId: "apply", output: "sql`SELECT name FROM users WHERE id = ${id}`" }],
        },
      ],
    },
    {
      name: "offsets after an interpolation stay in place",
      options: options(),
      code: "sql`SELECT id FROM users WHERE name = ${longExpression.name} AND emial = ''`",
      errors: [
        {
          messageId: "sql",
          column: 66,
          endColumn: 71,
          suggestions: [{ messageId: "apply", output: "sql`SELECT id FROM users WHERE name = ${longExpression.name} AND email = ''`" }],
        },
      ],
    },
    {
      name: "lines and escapes before the mistake",
      options: options(),
      code: String.raw`sql${"`"}
  SELECT '\u{1F600}\\\`' AS x
  FROM users
  WHERE emial = ${"${email}"}${"`"}`,
      errors: [{ messageId: "sql", line: 4, column: 9, endLine: 4, endColumn: 14, suggestions: 1 }],
    },
    {
      name: "an unknown table",
      options: options(),
      code: "sql`SELECT id FROM people`",
      errors: [{ messageId: "sql", column: 20, endColumn: 26 }],
    },
    {
      name: "a query that does not parse",
      options: options(),
      code: "sql`SELEC id FROM users`",
      errors: [{ messageId: "sql", column: 5, endColumn: 24 }],
    },
    {
      name: "node-postgres and postgres.js differ on numeric arrays",
      options: options({ driver: "pg" }),
      code: "sql`SELECT balance, scores, tags, created_at FROM users`",
      output: "sql<{ balance: string; scores: number[] | null; tags: string[]; created_at: Date }>`SELECT balance, scores, tags, created_at FROM users`",
      errors: [{ messageId: "missingTypeAnnotation" }],
    },
    {
      name: "postgres.js keeps numeric[] as strings",
      options: options({ driver: "postgres" }),
      code: "sql`SELECT scores FROM users`",
      output: "sql<{ scores: string[] | null }>`SELECT scores FROM users`",
      errors: [{ messageId: "missingTypeAnnotation" }],
    },
    {
      name: "overrides, transform and fieldTransform",
      options: options({
        overrides: { types: { BIGINT: "bigint" } },
        targets: [{ tag: "sql", transform: "{type}[]", fieldTransform: "camel" }],
      }),
      code: "sql`SELECT id, created_at FROM users`",
      output: "sql<{ id: bigint; createdAt: Date }[]>`SELECT id, created_at FROM users`",
      errors: [{ messageId: "missingTypeAnnotation" }],
    },
    {
      name: "suggest instead of fixing",
      options: options({ enforceType: "suggest" }),
      code: "sql`SELECT id FROM users`",
      errors: [{ messageId: "missingTypeAnnotation", suggestions: [{ messageId: "annotate", output: "sql<{ id: string }>`SELECT id FROM users`" }] }],
    },
    {
      name: "a rule set to warn",
      options: options({ rules: { "unknown-column": "warn" } }),
      code: "sql`SELECT nope FROM users`",
      errors: [{ messageId: "sql" }],
    },
  ],
});
