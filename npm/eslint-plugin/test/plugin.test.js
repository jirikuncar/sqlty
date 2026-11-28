import assert from "node:assert/strict";
import { test } from "node:test";
import { ESLint } from "eslint";
import plugin from "@sqlty/eslint-plugin";
import metadata from "../package.json" with { type: "json" };

test("the plugin names itself as its package does", () => {
  assert.deepEqual(plugin.meta, { name: "@sqlty/eslint-plugin", version: metadata.version });
});

test("ESLint loads it from a flat config", async () => {
  const eslint = new ESLint({ overrideConfigFile: true, overrideConfig: { plugins: { sqlty: plugin } } });
  const [result] = await eslint.lintText("const x = 1;");
  assert.equal(result.errorCount, 0);
});
