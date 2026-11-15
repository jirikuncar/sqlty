import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { before, test } from "node:test";

import { loadEngine } from "../../engine";
import { type Engine, project } from "../../web/config";
import { Uri } from "./vscode";

let engine: Engine;

before(async () => {
  engine = await loadEngine(Uri.file(resolve(".")) as never);
});

/** A new directory holding `files`, by path relative to it. */
function tree(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "sqlty-config-"));
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  }
  return root;
}

const at = (root: string, path = ".") => project(engine, Uri.file(join(root, path)) as never);

test("the closest table wins, and a file without one is passed over", async () => {
  const root = tree({
    "pyproject.toml": "[tool.sqlty]\ndialect = 'sqlite'\n",
    "app/pyproject.toml": "[project]\nname = 'app'\n",
    "app/deep/.keep": "",
  });
  assert.deepEqual(await at(root, "app/deep"), { settings: { dialect: "sqlite" }, schema: "" });
  assert.deepEqual(await at(tree({})), { settings: {}, schema: "" });
});

test("schema reads files, directories and globs in order, once each", async () => {
  const root = tree({
    "sqlty.toml": "schema = ['b.sql', 'dir', 'g/*', 'missing.sql', 'b.sql']\n",
    "b.sql": "B",
    "dir/z.sql": "Z",
    "dir/a/y.sql": "Y",
    "dir/notes.txt": "-",
    "g/2.sql": "G2",
    "g/1.sql": "G1",
    "g/sub/x.sql": "-",
  });
  assert.deepEqual(await at(root), {
    settings: { schema: ["b.sql", "dir", "g/*", "missing.sql", "b.sql"] },
    schema: "B\nY\nZ\nG1\nG2",
  });
});

test("extend applies a base under the table, rules and types merged", async () => {
  const root = tree({
    "base/sqlty.toml":
      "dialect = 'sqlite'\nschema = 'one.sql'\n[rules]\nselect-star = 'error'\nextra-param = 'warn'\n[types]\nx = 'int'\n",
    "base/one.sql": "ONE",
    "app/sqlty.toml":
      "extend = '../base/sqlty.toml'\n[rules]\nselect-star = 'ignore'\n[types]\ny = 'str'\n",
  });
  assert.deepEqual(await at(root, "app"), {
    settings: {
      dialect: "sqlite",
      schema: "one.sql",
      rules: { "select-star": "ignore", "extra-param": "warn" },
      types: { x: "int", y: "str" },
    },
    schema: "ONE",
  });
});

test("a broken table, a missing, empty or cyclic extend is an error", async () => {
  const errors = async (files: Record<string, string>) => {
    const found = await at(tree(files));
    assert.ok("error" in found);
    return found.error.replace(/\/.*\/sqlty-config-\w+/g, "ROOT");
  };
  assert.match(await errors({ "sqlty.toml": "schema = [" }), /TOML parse error/);
  assert.equal(
    await errors({ "sqlty.toml": "extend = 'gone.toml'" }),
    "extend = 'gone.toml': config file does not exist: ROOT/gone.toml",
  );
  assert.equal(
    await errors({ "sqlty.toml": "extend = 'pyproject.toml'", "pyproject.toml": "" }),
    "ROOT/pyproject.toml: no [tool.sqlty] table",
  );
  assert.equal(
    await errors({ "sqlty.toml": "extend = 'a.toml'", "a.toml": "extend = 'a.toml'" }),
    "ROOT/a.toml: extend = 'a.toml' extends itself in a cycle",
  );
});
