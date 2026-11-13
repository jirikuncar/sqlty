import * as assert from "node:assert/strict";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as vscode from "vscode";

import { encode, session, toml } from "../playground";
import {
  COMPLETION_VERSION,
  compareVersions,
  completionNotice,
  findServer,
  MINIMUM_VERSION,
  type Settings,
} from "../server";

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
      "https://sqlty.kuncar.dev/reference/rules/#narrow-type",
    );

    const actions = await codeActions(uri, diagnostic.range, "source.fixAll.sqlty");
    const fixAll = actions.find((action) => action.kind?.value === "source.fixAll.sqlty");
    assert.ok(fixAll?.edit, "a fix-all action with an edit");
    assert.ok(await vscode.workspace.applyEdit(fixAll.edit));
    assert.match(document.getText(), /SQL\[tuple\[float \| None\]\]/);
    await diagnostics(uri, (found) => found.length === 0);
  });

  test("completes the columns of a table in a query string", async () => {
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "complete.py");
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);
    // `stale-rewrite` of the unwrapped string: the buffer has been checked.
    await diagnostics(uri, (found) => found.length > 0);

    const position = document.positionAt(document.getText().indexOf("u.") + 2);
    const sqlty = async () => {
      const list = await vscode.commands.executeCommand<vscode.CompletionList>(
        "vscode.executeCompletionItemProvider",
        uri,
        position,
        ".",
      );
      return list.items.filter((item) => item.sortText?.match(/^\d{4}$/));
    };
    let items = await sqlty();
    for (let tries = 0; items.length === 0 && tries < 50; tries++) {
      await new Promise((resolve) => setTimeout(resolve, 100));
      items = await sqlty();
    }
    assert.deepEqual(
      items.map((item) => item.label),
      ["id", "name", "score"].map((label) => ({ label, description: "u" })),
    );
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

    const actions = await codeActions(uri, diagnostic.range, vscode.CodeActionKind.QuickFix.value);
    const fix = actions.find((action) => action.title === "Fix unknown-column");
    assert.ok(fix?.edit, "a quick fix with an edit");
    assert.ok(await vscode.workspace.applyEdit(fix.edit));
    assert.match(document.getText(), /SELECT id, name FROM users/);
  });

  test("highlights the SQL of marked strings, and no other string", async () => {
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "highlight.py");
    const tokens = await vscode.commands.executeCommand<{ c: string; t: string }[]>(
      "_workbench.captureSyntaxTokens",
      uri,
    );
    const sql = (scope: string) =>
      tokens.filter((token) => token.t.split(" ").includes(scope)).map((token) => token.c);
    const keywords = ["SELECT", "FROM", "WHERE", "SELECT", "AS", "FROM", "SELECT", "FROM", "WHERE"];
    const interpolated = ["SELECT", "FROM", "SELECT", "FROM", "WHERE", "LIMIT"];
    interpolated.push("SELECT", "FROM", "SELECT", "FROM");
    assert.deepEqual(sql("keyword.other.sql"), [...keywords, "SELECT", "FROM", ...interpolated]);
    const comments = ["--sql", "-- note", "-- sql", "--sql", "--sql"];
    comments.push("--sql", "--sql", "--sql", "--sql");
    assert.deepEqual(sql("comment.line.double-dash.sql"), comments);
    assert.deepEqual(sql("comment.block.sql"), ["/* the", "key */"]);
    assert.deepEqual(sql("string.quoted.single.sql").join(""), "'it''s''{{literal}}'");
    const fields = tokens.filter(
      (token) =>
        token.t.includes("meta.embedded.sql") &&
        token.t.includes("constant.character.format.placeholder.other.python"),
    );
    assert.equal(fields.map((token) => token.c).join(""), "{}{}{}{}{}");
    const expressions = tokens.filter((token) => /\b(user_id|limit)\b/.test(token.c));
    assert.ok(expressions.every((token) => !token.t.includes("keyword.other.sql")));
    assert.deepEqual(sql("variable.parameter.sql"), ["%(id)s", "$1"]);
    assert.deepEqual(sql("constant.numeric.sql"), ["1"]);
    const unmarked = ["SELECT plain", "SELECT raw", "SELECT data", "unmarked_"];
    const outside = tokens.filter(
      (token) =>
        token.t.includes("meta.embedded.sql") && unmarked.some((name) => token.c.includes(name)),
    );
    assert.deepEqual(outside, []);
  });

  test("wraps and strips the queries of a file from the Command Palette", async () => {
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "wrap.py");
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);
    const plain = document.getText();
    await diagnostics(uri, (found) => found.length > 0);

    await vscode.commands.executeCommand("sqlty.rewrite");
    assert.match(
      document.getText(),
      /^q = SQL\[tuple\[int \| None\]\]\("--sql\\nSELECT id FROM users"\)$/m,
    );
    await vscode.commands.executeCommand("sqlty.strip");
    assert.equal(document.getText(), plain);
    await vscode.commands.executeCommand("workbench.action.files.revert");
  });

  test("registers every command it contributes", async () => {
    const extension = vscode.extensions.getExtension("jirikuncar.sqlty");
    const contributed: { command: string; category: string }[] =
      extension?.packageJSON.contributes.commands;
    const registered = new Set(await vscode.commands.getCommands(true));
    assert.deepEqual(
      contributed.filter(({ command }) => !registered.has(command)),
      [],
    );
    assert.deepEqual(new Set(contributed.map(({ category }) => category)), new Set(["sqlty"]));
  });

  test("declares limited support for Restricted Mode", () => {
    const extension = vscode.extensions.getExtension("jirikuncar.sqlty");
    assert.equal(extension?.packageJSON.capabilities.untrustedWorkspaces.supported, "limited");
  });
});

suite("findServer", () => {
  const settings = (path: string[]): Settings => ({
    path,
    interpreter: [],
    breaking: { against: "", direction: "both" },
    completion: { enable: true },
  });

  test("tells when the server is too old to complete", () => {
    const on = settings([]);
    const off = { ...on, completion: { enable: false } };
    assert.equal(completionNotice(on, true, COMPLETION_VERSION), undefined);
    assert.equal(completionNotice(off, false, MINIMUM_VERSION), undefined);
    assert.equal(
      completionNotice(on, false, MINIMUM_VERSION),
      `sqlty: completion needs sqlty ${COMPLETION_VERSION} or newer, found ${MINIMUM_VERSION}. ` +
        `Upgrade it: \`uv add --dev 'sqlty>=${COMPLETION_VERSION}'\` or \`pip install -U sqlty\`, ` +
        "or set sqlty.completion.enable to false.",
    );
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

suite("playground", () => {
  test("shares the settings the playground takes, and the dialect beside them", () => {
    const settings = {
      dialect: "sqlite",
      schema: "schema.sql",
      "import-style": "import sqlty",
      rules: { "select-star": "error", "stale-rewrite": "ignore" },
    };
    assert.deepEqual(session("q = 1\n", { settings, schema: "CREATE TABLE t (id INT);" }), {
      source: "q = 1\n",
      schema: "CREATE TABLE t (id INT);",
      dialect: "sqlite",
      config:
        'import-style = "import sqlty"\n\n[rules]\nselect-star = "error"\nstale-rewrite = "ignore"\n',
    });
  });

  test("writes nested tables and quoted keys", () => {
    assert.equal(
      toml({ types: { "my type": "int" }, a: [1, "b"] }),
      'a = [1, "b"]\n\n[types]\n"my type" = "int"\n',
    );
  });

  test("encodes a session as share.js decodes it", async () => {
    const shared = session("q = 1\n", { settings: {}, schema: "" });
    const hash = await encode(shared);
    assert.match(hash, /^[A-Za-z0-9_-]+$/);
    const bytes = Buffer.from(hash, "base64url");
    const inflated = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("deflate"));
    assert.deepEqual(JSON.parse(await new Response(inflated).text()), shared);
  });
});

/** An executable that prints `sqlty <version>` for `--version`, like `sqlty --version`. */
function fakeSqlty(version: string): string {
  const path = join(mkdtempSync(join(tmpdir(), "sqlty-")), "sqlty");
  writeFileSync(path, `#!/bin/sh\necho "sqlty ${version}"\n`);
  chmodSync(path, 0o755);
  return path;
}

/** The code actions of `kind` at `range`, asked again while VS Code cancels them. */
async function codeActions(
  uri: vscode.Uri,
  range: vscode.Range,
  kind: string,
): Promise<vscode.CodeAction[]> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await vscode.commands.executeCommand<vscode.CodeAction[]>(
        "vscode.executeCodeActionProvider",
        uri,
        range,
        kind,
      );
    } catch (error) {
      // VS Code cancels the request when an extension registers a code action provider meanwhile.
      if (!(error instanceof Error && error.name === "Canceled") || attempt === 50) {
        throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
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
