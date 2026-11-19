import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { beforeEach, test } from "node:test";

import { type Host, registerCommands, RULES } from "../../commands";
import type { Engine } from "../../web/config";
import {
  calls,
  commands,
  Diagnostic,
  DiagnosticSeverity,
  env,
  extensions,
  languages,
  output,
  Position,
  Range,
  ui,
  UIKind,
  Uri,
  window,
} from "./vscode";

/** The engine answering `config` with `settings` and nothing else. */
const engine =
  (settings: Record<string, unknown> | null): Engine =>
  () =>
    settings === null ? null : { config: settings };

function host(overrides: Partial<Host> = {}): Host {
  return {
    output: output(),
    engine: async () => engine(null),
    unsupported: () => undefined,
    describe: () => ["Server: test"],
    rules: async () => undefined,
    ...overrides,
  };
}

function editor(text: string, at = new Position(0, 0)): void {
  const uri = Uri.file(join(mkdtempSync(join(tmpdir(), "sqlty-commands-")), "a.py"));
  window.activeTextEditor = {
    document: { uri, lineCount: 1, getText: () => text },
    selection: { active: at },
  };
}

function found(rule: string, line = 0): Diagnostic {
  const diagnostic = new Diagnostic(
    new Range(new Position(line, 0), new Position(line, 9)),
    "m",
    DiagnosticSeverity.Warning,
  );
  diagnostic.source = "sqlty";
  diagnostic.code = { value: rule, target: Uri.parse(RULES) };
  return diagnostic;
}

beforeEach(() => {
  calls.length = 0;
  window.activeTextEditor = undefined;
  languages.diagnostics = [];
});

test("a source action applies the edit the server offers", async () => {
  const registered = registerCommands(host({ unsupported: (kind) => `no ${kind}` }));
  await commands.executeCommand("sqlty.fixAll");
  assert.deepEqual(calls.at(-1), ["showErrorMessage", "no source.fixAll.sqlty"]);
  registered.dispose();

  const disposable = registerCommands(host());
  await commands.executeCommand("sqlty.format");
  editor("x");
  const edit = { changes: 1 };
  const offered = [
    { kind: { value: "source.format.sqlty" } },
    { kind: undefined, edit },
    { kind: { value: "source.format.sqlty" }, edit },
  ];
  const provider = commands.registerCommand("vscode.executeCodeActionProvider", () => offered);
  await commands.executeCommand("sqlty.format");
  assert.deepEqual(calls.at(-1), ["workspace.applyEdit", edit]);
  offered.pop();
  await commands.executeCommand("sqlty.format");
  assert.deepEqual(calls.at(-1), [
    "showInformationMessage",
    "sqlty: source.format.sqlty changes nothing in this file.",
  ]);
  provider.dispose();
  disposable.dispose();
});

test("debug information goes to the log and the clipboard", async () => {
  const disposable = registerCommands(host());
  await commands.executeCommand("sqlty.showLogs");
  await commands.executeCommand("sqlty.debugInformation");
  const text = calls.find(([name]) => name === "clipboard.writeText")![1] as string;
  assert.equal(
    text,
    "sqlty extension: unknown\nFake Code: 1.0.0 (desktop)\nWorkspace trusted: true\nServer: test",
  );
  const getExtension = extensions.getExtension;
  extensions.getExtension = () => ({ packageJSON: { version: "0.6.0" } });
  env.uiKind = UIKind.Web;
  await commands.executeCommand("sqlty.debugInformation");
  assert.match(calls.at(-2)![1] as string, /^sqlty extension: 0\.6\.0\nFake Code: 1\.0\.0 \(web\)/);
  extensions.getExtension = getExtension;
  env.uiKind = UIKind.Desktop;
  disposable.dispose();
});

test("explain rule offers the rules here first, and opens the page picked", async () => {
  const disposable = registerCommands(host());
  const offered: { label: string; picked: boolean }[][] = [];
  ui.quickPick = (items) => {
    offered.push(items as { label: string; picked: boolean }[]);
    return (items as unknown[])[0];
  };
  editor("x", new Position(1, 2));
  const uri = (window.activeTextEditor as { document: { uri: Uri } }).document.uri;
  const other = Uri.file("/other.py");
  const plain = found("plain");
  plain.code = "select-star";
  const foreign = found("foreign", 1);
  foreign.source = "ruff";
  languages.diagnostics = [
    [uri, [found("unknown-column"), found("extra-param", 1), foreign]],
    [other, [plain, found("extra-param")]],
  ];
  await commands.executeCommand("sqlty.explainRule");
  assert.deepEqual(
    offered[0].map((item) => [item.label, item.picked]),
    [
      ["extra-param", true],
      ["select-star", false],
      ["unknown-column", false],
      ["All rules", false],
    ],
  );
  assert.deepEqual(calls.at(-1), ["openExternal", `${RULES}extra-param/`]);

  window.activeTextEditor = undefined;
  ui.quickPick = (items) => (items as unknown[]).at(-1);
  disposable.dispose();
  const listed = registerCommands(
    host({ rules: async () => [{ name: "select-star", severity: "warn", description: "" }] }),
  );
  await commands.executeCommand("sqlty.explainRule");
  assert.deepEqual(calls.at(-1), ["openExternal", RULES]);
  calls.length = 0;
  ui.quickPick = () => undefined;
  await commands.executeCommand("sqlty.explainRule");
  assert.deepEqual(calls, [["executeCommand", "sqlty.explainRule"]]);
  listed.dispose();
});

test("open in playground shares the file and its project as a link", async () => {
  const disposable = registerCommands(
    host({ engine: async () => engine({ dialect: "sqlite", driver: "aiosqlite" }) }),
  );
  await commands.executeCommand("sqlty.openInPlayground");
  assert.deepEqual(calls, [["executeCommand", "sqlty.openInPlayground"]]);
  editor("x = 1");
  const { uri } = (window.activeTextEditor as { document: { uri: Uri } }).document;
  writeFileSync(join(uri.fsPath, "..", "sqlty.toml"), "");
  await commands.executeCommand("sqlty.openInPlayground");
  const link = calls.at(-1)![1] as string;
  assert.match(link, /^https:\/\/sqlty\.kuncar\.dev\/playground\/#[\w-]+$/);
  assert.deepEqual(calls.at(-2), ["clipboard.writeText", link]);
  disposable.dispose();

  const failing = registerCommands(
    host({ engine: async () => () => ({ error: "broken config" }) }),
  );
  await commands.executeCommand("sqlty.openInPlayground");
  assert.deepEqual(calls.at(-1), ["showErrorMessage", "sqlty: broken config"]);
  failing.dispose();

  const large = registerCommands(host());
  editor(Array.from({ length: 4000 }, (_, i) => `x${i * 7919} = ${i * 104729}`).join("\n"));
  await commands.executeCommand("sqlty.openInPlayground");
  assert.match(calls.at(-1)![1] as string, /too large to share as a link/);
  large.dispose();
});
