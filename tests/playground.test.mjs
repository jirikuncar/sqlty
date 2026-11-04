// The docs playground's engine (docs/assets/playground/engine.js) on the WebAssembly build:
// `make playground`, then `node --test tests/playground.test.mjs`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { load } from "../docs/assets/playground/engine.js";

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
