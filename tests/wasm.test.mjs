// @sqlty/wasm (npm/wasm/index.js) finding its module: `make wasm`, then
// `node --test tests/wasm.test.mjs`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { load } from "../npm/wasm/index.js";

const wasm = new URL("../npm/wasm/sqlty_wasm.wasm", import.meta.url);
const request = { op: "analyze", source: 'q = "--sql\\nSELECT 1 AS n"' };
const typed = async (ask) => assert.equal((await ask)(request).queries[0].row_type, "tuple[int]");

test("the module next to the package", () => typed(load()));

test("a file path as a URL", () => typed(load(wasm.href)));

test("bytes", async () => typed(load(await readFile(wasm))));

test("a response, or a promise of one", async () => {
  const bytes = await readFile(wasm);
  await typed(load(new Response(bytes)));
  await typed(load(Promise.resolve(new Response(bytes))));
});

test("a URL to fetch", async () => {
  const url = `data:application/wasm;base64,${(await readFile(wasm)).toString("base64")}`;
  await typed(load(url));
});

test("a failed response", async () => {
  await assert.rejects(load(new Response("", { status: 404, statusText: "Not Found" })), {
    message: "sqlty_wasm.wasm: 404 Not Found",
  });
});

test("the core module leaves out sqruff", async () => {
  const core = await load(new URL("../npm/wasm/sqlty_wasm_core.wasm", import.meta.url));
  await typed(core);
  const needs = { error: "the core module leaves out sqruff", needs: "sqruff" };
  assert.deepEqual(core({ ...request, op: "format" }), needs);
  assert.deepEqual(core({ ...request, config: 'rules = { sqruff = "warn" }' }), needs);
  assert.equal(core({ ...request, op: "complete", offset: 0 }), null);
});
