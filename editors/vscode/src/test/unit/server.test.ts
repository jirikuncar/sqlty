import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { compareVersions, completionNotice, findServer, type Settings } from "../../server";

const settings: Settings = {
  path: [],
  interpreter: [],
  breaking: { against: "", direction: "both" },
  completion: { enable: true },
};

/** An executable in a new directory that prints `output` for `--version`. */
function script(name: string, output: string): string {
  const path = join(mkdtempSync(join(tmpdir(), "sqlty-server-")), name);
  writeFileSync(path, `#!/bin/sh\necho "${output} $*"\n`);
  chmodSync(path, 0o755);
  return path;
}

test("sqlty.path wins over the interpreter and PATH", async () => {
  const sqlty = script("sqlty", "sqlty 0.6.0");
  const found = await findServer({ ...settings, path: [sqlty] }, [script("python", "sqlty 9.0")]);
  assert.deepEqual(found, {
    server: { command: sqlty, args: ["server"], origin: "sqlty.path", version: "0.6.0" },
  });
});

test("the interpreter runs sqlty as a module", async () => {
  const python = script("python", "sqlty 0.6.0");
  const found = await findServer({ ...settings, path: ["~/missing", "~"] }, [python, "-I"]);
  assert.deepEqual(found, {
    server: {
      command: python,
      args: ["-I", "-m", "sqlty", "server"],
      origin: "interpreter",
      version: "0.6.0",
    },
  });
});

test("a sqlty too old is named, one that is not sqlty is skipped", async () => {
  const old = script("sqlty", "sqlty 0.2.9");
  const other = script("other", "other 1.0");
  const path = process.env.PATH;
  process.env.PATH = "";
  try {
    const found = await findServer({ ...settings, path: [other, old] }, []);
    assert.ok("error" in found);
    assert.match(
      found.error,
      /needs sqlty 0\.3\.0 or newer, found 0\.2\.9 \(.*sqlty, from sqlty\.path\)/,
    );
    assert.deepEqual(await findServer(settings, undefined), {
      error: "sqlty: no sqlty found. Install sqlty in the selected interpreter, or set sqlty.path.",
    });
  } finally {
    process.env.PATH = path;
  }
});

test("versions compare by release, a pre-release as its release", () => {
  assert.equal(compareVersions("0.3.0", "0.3"), 0);
  assert.equal(compareVersions("0.3.0a1", "0.3.0"), 0);
  assert.equal(compareVersions("0.10.0", "0.9.9"), 1);
  assert.equal(compareVersions("0.2.0.dev2", "0.3.0"), -1);
  assert.equal(compareVersions("dev", "0.0.0"), 0);
});

test("completion is noticed only when it is on and not advertised", () => {
  assert.match(completionNotice(settings, false, "0.3.0")!, /completion needs sqlty 0\.4\.0/);
  assert.equal(completionNotice(settings, true, "0.4.0"), undefined);
  assert.equal(
    completionNotice({ ...settings, completion: { enable: false } }, false, "0.3.0"),
    undefined,
  );
});
