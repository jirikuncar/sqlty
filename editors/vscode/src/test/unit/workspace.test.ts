import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { beforeEach, test } from "node:test";

import * as vscode from "vscode";

import { breakingAgainst, checkWorkspace, rules } from "../../workspace";
import { calls, diagnostics, output, ui, Uri, workspace } from "./vscode";

/** A `sqlty` that prints `stdout` and exits with `code`. */
function cli(stdout: string, code = 0): { command: string; args: string[] } {
  const path = join(mkdtempSync(join(tmpdir(), "sqlty-cli-")), "sqlty");
  writeFileSync(path, `#!/bin/sh\ncat <<'EOF'\n${stdout}\nEOF\nexit ${code}\n`);
  chmodSync(path, 0o755);
  return { command: path, args: [] };
}

function folder(): string {
  return mkdtempSync(join(tmpdir(), "sqlty-folder-"));
}

beforeEach(() => {
  calls.length = 0;
  ui.cancelled = false;
  workspace.textDocuments = [];
});

test("rules reads the lines of sqlty rule", async () => {
  const listed = cli("select-star  warn  SELECT * hides columns\nnotarule");
  assert.deepEqual(await rules(listed), [
    { name: "select-star", severity: "warn", description: "SELECT * hides columns" },
  ]);
  assert.equal(await rules(cli("", 2)), undefined);
  assert.equal(await rules({ command: "/nonexistent/sqlty", args: [] }), undefined);
});

test("the workspace check reports files that are not open", async () => {
  const root = folder();
  const open = Uri.file(join(root, "open.py"));
  workspace.textDocuments = [{ uri: open }];
  workspace.workspaceFolders = [{ uri: Uri.file(root), name: "root" }];
  const found = [
    { file: "a.py", line: 2, rule: "select-star", severity: "warn", message: "m" },
    {
      file: join(root, "a.py"),
      rule: "unknown-column",
      severity: "error",
      message: "n",
      start: { line: 3, column: 5 },
      end: { line: 3, column: 9 },
    },
    { file: "b.py", rule: "r", severity: "warn", message: "o" },
    { file: "open.py", line: 1, rule: "r", severity: "warn", message: "p" },
  ];
  const collection = diagnostics();
  await checkWorkspace(cli(JSON.stringify(found), 1), collection, output());
  const a = collection.entries.get(Uri.file(join(root, "a.py")).toString())!;
  assert.deepEqual(
    a.map((d) => [d.range.start.line, d.range.start.character, d.range.end.character, d.severity]),
    [
      [1, 0, 0, vscode.DiagnosticSeverity.Warning],
      [2, 4, 8, vscode.DiagnosticSeverity.Error],
    ],
  );
  assert.equal(
    collection.entries.get(Uri.file(join(root, "b.py")).toString())![0].range.start.line,
    0,
  );
  assert.equal(collection.entries.size, 2);
  assert.deepEqual(calls.at(-1), [
    "showInformationMessage",
    "sqlty: 3 problem(s) in 2 file(s) outside the open files; see Problems.",
  ]);

  await checkWorkspace(cli("[]"), collection, output());
  assert.deepEqual(calls.at(-1), [
    "showInformationMessage",
    "sqlty: no problems outside the open files.",
  ]);
});

test("a check that fails, does not run or is cancelled says so", async () => {
  workspace.workspaceFolders = [{ uri: Uri.file(folder()), name: "root" }];
  await checkWorkspace(cli("Traceback", 2), diagnostics(), output());
  assert.deepEqual(calls.slice(1, 2), [
    ["showErrorMessage", "sqlty: checking root failed; see the logs."],
  ]);

  calls.length = 0;
  await checkWorkspace({ command: "/nonexistent/sqlty", args: [] }, diagnostics(), output());
  assert.equal(calls[0][0], "output.error");

  calls.length = 0;
  ui.cancelled = true;
  await checkWorkspace(cli("[]"), diagnostics(), output());
  ui.cancelled = "kill";
  await checkWorkspace(cli("[]"), diagnostics(), output());
  assert.deepEqual(calls, []);

  workspace.workspaceFolders = undefined;
  ui.cancelled = false;
  await checkWorkspace(cli("[]"), diagnostics(), output());
  assert.deepEqual(calls.at(-1), [
    "showInformationMessage",
    "sqlty: no problems outside the open files.",
  ]);
});

test("breaking against takes a ref, a typed revision or a browsed file", async () => {
  const root = folder();
  const git = (...args: string[]) => execFileSync("git", args, { cwd: root, stdio: "ignore" });
  git("init", "-q", "-b", "main");
  git("-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "a");
  git("tag", "v1");
  mkdirSync(join(root, "schemas"));
  workspace.workspaceFolders = [{ uri: Uri.file(root), name: "root" }];
  const labels: string[][] = [];
  ui.quickPick = (items) => {
    const listed = items as { label: string; value?: string; action?: string }[];
    labels.push(listed.map((item) => item.label));
    return listed.find((item) => item.value === "v1");
  };
  await breakingAgainst();
  assert.deepEqual(labels[0].slice(3), ["Git", "$(git-branch) main", "$(git-branch) v1"]);

  ui.quickPick = (items) => (items as { action?: string }[]).find((item) => item.action === "type");
  ui.inputBox = () => "HEAD~1";
  await breakingAgainst();

  ui.quickPick = (items) =>
    (items as { action?: string }[]).find((item) => item.action === "browse");
  ui.openDialog = () => [Uri.file(join(root, "schemas"))];
  await breakingAgainst();
  ui.openDialog = () => [Uri.file(root)];
  await breakingAgainst();
  ui.openDialog = () => undefined;
  await breakingAgainst();
  ui.quickPick = () => undefined;
  await breakingAgainst();

  assert.deepEqual(
    calls.filter(([name]) => name === "configuration.update").map(([, , value]) => value),
    ["v1", "HEAD~1", "schemas", "."],
  );
});

test("breaking against needs a folder, and lists no refs outside a repository", async () => {
  workspace.workspaceFolders = undefined;
  await breakingAgainst();
  workspace.workspaceFolders = [{ uri: Uri.file(folder()), name: "root" }];
  ui.quickPick = (items) => {
    assert.equal((items as unknown[]).length, 4);
    return undefined;
  };
  await breakingAgainst();
  workspace.workspaceFolders = [{ uri: Uri.file("/nonexistent"), name: "gone" }];
  await breakingAgainst();
  assert.deepEqual(calls, []);
});
