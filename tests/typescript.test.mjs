// The TypeScript mode of the docs playground: @sqlty/eslint-plugin's checks (core.js and
// types.js, copied by `make playground`) on the tagged templates typescript.js reads.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { load } from "../npm/wasm/engine.js";
import { compose, cook, rowType } from "../docs/assets/playground/core.js";
import { drivers, lint, templates, tsType } from "../docs/assets/playground/typescript.js";

const engine = await load(await readFile(new URL("../npm/wasm/sqlty_wasm.wasm", import.meta.url)));
const text = `CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT NOT NULL, email TEXT, scores NUMERIC[]);
CREATE TYPE mood AS ENUM ('ok');
CREATE TABLE moods (id INT PRIMARY KEY, moods mood[] NOT NULL, spent INTERVAL);`;
const schema = { key: "test", text: () => text };
const settings = (more = {}) => ({ engine, schema, target: { tag: "sql" }, ...more });

test("only the sql tagged templates outside comments and strings are read", () => {
  const source = [
    "// sql`SELECT 1` in a comment",
    "/* sql`SELECT 2` */ const s = 'sql`SELECT 3`' + \"\\\"sql`\";",
    "other`SELECT 4`; `SELECT 5`; (x>`6`);",
    "db.sql`SELECT 7`;",
    "sql<{ a: Map<string, number> }>`SELECT ${f({ a: '}' })} ${`n${1}`} 8`;",
    "/* unterminated",
  ].join("\n");
  const found = templates(source);
  assert.equal(found.length, 1);
  assert.equal(found[0].typeArguments.inner, "{ a: Map<string, number> }");
  assert.deepEqual(
    found[0].expressions.map(({ start, end }) => source.slice(start, end)),
    ["f({ a: '}' })", "`n${1}`"],
  );
  assert.deepEqual(templates("x > y`z`"), []);
  assert.deepEqual(templates("sql`SELECT ${unclosed")[0].expressions, [{ start: 13, end: 21 }]);
});

test("a missing and a stale row type, an unknown column and its fix, on their lines", () => {
  const source = [
    "const a = sql`SELECT id, name FROM users WHERE id = ${id}`;",
    "const b = sql<{ id: number }>`SELECT id FROM users`;",
    "const c = sql<{ name: string }>`SELECT name FROM users`;",
    "const d = sql`SELECT nme FROM users`;",
    "const e = sql`SELECT * FROM ${sql(table)}`;",
    "const f = sql`DELETE FROM users WHERE id = ${id}`;",
  ].join("\n");
  const found = lint(source, settings());
  assert.deepEqual(
    found.diagnostics.map(({ rule, line, missing }) => [rule, line, missing]),
    [["row-type", 1, true], ["row-type", 2, false], ["unknown-column", 4, undefined]],
  );
  assert.equal(found.diagnostics[0].fix[0].replacement, "<{ id: string; name: string }>");
  assert.equal(found.diagnostics[2].fix[0].replacement, "name");
  assert.deepEqual(found.queries.map(({ line }) => line), [1, 2, 3, 4, 6]);
  assert.equal(found.queries[0].params[0].sql_type, "BIGINT");
});

test("skipTypeAnnotations reports the SQL only, and an engine error stops the check", () => {
  const source = "sql`SELECT id FROM users`";
  assert.deepEqual(lint(source, settings({ target: { tag: "sql", skipTypeAnnotations: true } })).diagnostics, []);
  assert.match(lint(source, settings({ dialect: "nope" })).error, /nope/);
});

test("the schema is sent again once the engine no longer holds its key", () => {
  let sent = 0;
  const counting = { key: "fresh", text: () => (sent++, text) };
  lint("sql`SELECT id FROM users`", settings({ schema: counting }));
  lint("sql`SELECT id FROM users`", settings({ schema: counting }));
  assert.equal(sent, 1);
});

test("escapes, interpolations and line continuations map back to the source", () => {
  assert.deepEqual(cook(String.raw`a\n\x41\u0042\u{1F600}\`\
b`), { text: "a\nAB\u{1F600}`b", units: [0, 1, 3, 7, 13, 13, 22, 26, 27] });
  const { sql, range } = compose([{ raw: "SELECT ", start: 10 }, { raw: " AS x", start: 30 }], [{ start: 19, end: 28 }]);
  assert.equal(sql, "SELECT $1 AS x");
  assert.deepEqual(range(7, 9), [19, 28]);
  assert.deepEqual(range(10, 12), [31, 33]);
  assert.deepEqual(range(0, 0), [10, 11]);
});

test("row types follow the driver, the overrides, transform and fieldTransform", () => {
  const columns = [
    { name: "user_id", sql_type: "BIGINT", nullable: false },
    { name: "total", sql_type: "DECIMAL(10, 2)", nullable: true },
    { name: "two words", sql_type: null, nullable: null },
  ];
  assert.equal(
    rowType(columns, { transform: "{type}[]", fieldTransform: "camel" }, drivers.postgres),
    '{ userId: string; total: string | null; "two words": unknown }[]',
  );
  assert.equal(rowType(columns.slice(0, 1), { fieldTransform: "pascal" }, drivers.pg, { BIGINT: "bigint" }), "{ UserId: bigint }");
  assert.equal(rowType([{ name: "userId", sql_type: "INT" }], { fieldTransform: "screaming snake" }, drivers.pg), "{ USER_ID: number }");
  assert.equal(rowType([], {}, drivers.pg), "Record<string, never>");
});

test("arrays differ by driver", () => {
  const cases = [
    ["ARRAY<DECIMAL>", "number[]", "string[]"],
    ["ARRAY<MOOD>", "string", "string[]"],
    ["ARRAY<ARRAY<INT>>", "number[][]", "number[][]"],
    ["INTERVAL", 'import("postgres-interval").IPostgresInterval', "string"],
    ["ARRAY<INTERVAL>", 'import("postgres-interval").IPostgresInterval[]', "string[]"],
  ];
  for (const [sqlType, pg, postgres] of cases) {
    assert.equal(tsType(sqlType, drivers.pg), pg, sqlType);
    assert.equal(tsType(sqlType, drivers.postgres), postgres, sqlType);
  }
  assert.equal(tsType("ARRAY<UUID>", drivers.pg, { UUID: "Id | string" }), "(Id | string)[]");
});
