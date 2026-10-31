import * as assert from "node:assert/strict";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as vscode from "vscode";

import { compareVersions, findServer, MINIMUM_VERSION, type Settings } from "../server";

suite("sqlty", () => {
  test("shows the diagnostics of sqlty check, and fixes them", async () => {
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "q.py");
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);

    const [diagnostic] = await diagnostics(uri, (found) => found.length > 0);
    assert.equal(diagnostic.source, "sqlty");
    assert.equal(diagnostic.message, "SQL[tuple[int]] should be SQL[tuple[float | None]]");
    assert.deepEqual(diagnostic.range, new vscode.Range(2, 8, 2, 18));
    const code = diagnostic.code as { value: string; target: vscode.Uri };
    assert.equal(code.value, "narrow-type");
    assert.equal(
      code.target.toString(true),
      "https://jirikuncar.github.io/sqlty/reference/rules/#narrow-type",
    );

    const actions = await vscode.commands.executeCommand<vscode.CodeAction[]>(
      "vscode.executeCodeActionProvider",
      uri,
      diagnostic.range,
      "source.fixAll.sqlty",
    );
    const fixAll = actions.find((action) => action.kind?.value === "source.fixAll.sqlty");
    assert.ok(fixAll?.edit, "a fix-all action with an edit");
    assert.ok(await vscode.workspace.applyEdit(fixAll.edit));
    assert.match(document.getText(), /SQL\[tuple\[float \| None\]\]/);
    await diagnostics(uri, (found) => found.length === 0);
  });

  test("offers the closest column as the quick fix of unknown-column", async () => {
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "typo.py");
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);

    const [diagnostic] = await diagnostics(uri, (found) => found.length > 0);
    assert.equal(
      diagnostic.message,
      "column `surname` does not exist in `users`; did you mean `name`?",
    );

    const actions = await vscode.commands.executeCommand<vscode.CodeAction[]>(
      "vscode.executeCodeActionProvider",
      uri,
      diagnostic.range,
      vscode.CodeActionKind.QuickFix.value,
    );
    const fix = actions.find((action) => action.title === "Fix unknown-column");
    assert.ok(fix?.edit, "a quick fix with an edit");
    assert.ok(await vscode.workspace.applyEdit(fix.edit));
    assert.match(document.getText(), /SELECT id, name FROM users/);
  });
});

suite("findServer", () => {
  const settings = (path: string[]): Settings => ({
    path,
    interpreter: [],
    breaking: { against: "", direction: "both" },
  });

  test("compares releases, a pre-release as its release", () => {
    assert.equal(compareVersions("0.2.9", "0.3.0"), -1);
    assert.equal(compareVersions("0.3.0", "0.3.0"), 0);
    assert.equal(compareVersions("0.3", "0.3.0"), 0);
    assert.equal(compareVersions("0.3.0a1", "0.3.0"), 0);
    assert.equal(compareVersions("0.10.0", "0.9.0"), 1);
    assert.equal(compareVersions("1.0.0.dev2", "0.99.0"), 1);
  });

  test("skips a sqlty older than the minimum for a newer one further down", async () => {
    const old = fakeSqlty("0.2.2");
    const found = await findServer(settings([old, fakeSqlty(MINIMUM_VERSION)]), undefined);
    assert.ok("server" in found, JSON.stringify(found));
    assert.equal(found.server.version, MINIMUM_VERSION);
    assert.notEqual(found.server.command, old);
    assert.deepEqual(found.server.args, ["server"]);
  });

  test("runs the interpreter's sqlty as a module", async () => {
    const found = await findServer(settings([]), [fakeSqlty("99.0.0"), "-I"]);
    assert.ok("server" in found, JSON.stringify(found));
    assert.deepEqual(found.server.args, ["-I", "-m", "sqlty", "server"]);
    assert.equal(found.server.origin, "interpreter");
  });

  test("rejects a sqlty older than the minimum, naming it", async () => {
    const old = fakeSqlty("0.2.2");
    const path = process.env.PATH;
    process.env.PATH = "";
    try {
      const found = await findServer(settings([old, join(tmpdir(), "missing")]), undefined);
      assert.ok("error" in found, JSON.stringify(found));
      assert.equal(
        found.error,
        `sqlty: the extension needs sqlty ${MINIMUM_VERSION} or newer, found 0.2.2 (${old}, from sqlty.path). ` +
          `Upgrade it: \`uv add --dev 'sqlty>=${MINIMUM_VERSION}'\` or \`pip install -U sqlty\`.`,
      );
    } finally {
      process.env.PATH = path;
    }
  });
});

/** An executable that prints `sqlty <version>` for `--version`, like `sqlty --version`. */
function fakeSqlty(version: string): string {
  const path = join(mkdtempSync(join(tmpdir(), "sqlty-")), "sqlty");
  writeFileSync(path, `#!/bin/sh\necho "sqlty ${version}"\n`);
  chmodSync(path, 0o755);
  return path;
}

/** The diagnostics of `uri` once `done` holds for them. */
async function diagnostics(
  uri: vscode.Uri,
  done: (found: vscode.Diagnostic[]) => boolean,
): Promise<vscode.Diagnostic[]> {
  for (let attempt = 0; attempt < 300; attempt++) {
    const found = vscode.languages.getDiagnostics(uri);
    if (done(found)) {
      return found;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(
    `diagnostics of ${uri.toString()}: ${JSON.stringify(vscode.languages.getDiagnostics(uri))}`,
  );
}
