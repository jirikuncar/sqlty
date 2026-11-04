// The docs playground's engine (docs/assets/playground/engine.js) on the WebAssembly build:
// `make playground`, then `node --test tests/playground.test.mjs`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { load } from "../docs/assets/playground/engine.js";
import { decode, encode } from "../docs/assets/playground/share.js";

const ask = await load(await readFile(new URL("../docs/assets/playground/sqlty_wasm.wasm", import.meta.url)));
const schema = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);";

test("a query is typed against the schema", () => {
  const source = 'q = "--sql\\nSELECT id, name FROM users"';
  const answer = ask({ op: "analyze", source, schema, dialect: "postgres" });
  assert.equal(answer.queries[0].row_type, "tuple[int, str]");
});

test("a request larger than the memory the module starts with", () => {
  const columns = Array.from({ length: 20000 }, () => "name").join(", ");
  const source = `q = "--sql\\nSELECT ${columns} FROM users"`;
  const answer = ask({ op: "analyze", source, schema, dialect: "sqlite" });
  assert.equal(answer.queries[0].columns.length, 20000);
});

test("an unknown dialect is an error", () => {
  assert.match(ask({ op: "analyze", source: "", dialect: "postgress" }).error, /postgress/);
});

test("a configuration applies its dialect, import style and rules", () => {
  const source = 'q = "--sql\\nSELECT 1 = 1 AS b"';
  const config = 'dialect = "sqlite"\nimport-style = "import sqlty"\n[rules]\nstale-rewrite = "error"';
  const answer = ask({ op: "analyze", source, dialect: "postgres", config });
  assert.equal(answer.queries[0].row_type, "tuple[int]");
  assert.equal(answer.diagnostics[0].severity, "error");
  assert.match(answer.rewritten, /^import sqlty\n/);
});

test("a configuration key that needs files is an error", () => {
  const answer = ask({ op: "analyze", source: "", dialect: "postgres", config: 'schema = "schema.sql"' });
  assert.match(answer.error, /not 'schema'/);
});

test("a session survives its share link", async () => {
  const session = { source: "q = 'é'\n".repeat(200), schema, dialect: "mysql", config: "" };
  const hash = await encode(session);
  assert.match(hash, /^[A-Za-z0-9_-]+$/);
  assert.ok(hash.length < JSON.stringify(session).length / 4);
  assert.deepEqual(await decode(hash), session);
  assert.equal(await decode("not-a-session"), null);
});

test("a diagnostic carries its quick fix, and the source what fix and strip make", () => {
  const source = 'q = "--sql\\nSELECT nme FROM users"\n';
  const answer = ask({ op: "analyze", source, schema, dialect: "postgres" });
  const typo = answer.diagnostics.find((found) => found.rule === "unknown-column");
  assert.deepEqual(typo.fix, [{ start: 19, end: 22, replacement: "name" }]);
  assert.equal(answer.fixed, 'from sqlty import SQL\nq = SQL[tuple[str]]("--sql\\nSELECT name FROM users")\n');
  assert.equal(ask({ op: "analyze", source: answer.fixed, schema, dialect: "postgres" }).stripped, source.replace("nme", "name"));
});
