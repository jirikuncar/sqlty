import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { afterEach, beforeEach, test } from "node:test";
import { setTimeout } from "node:timers/promises";

import { activate, deactivate } from "../../extension";
import { server } from "./languageclient";
import { python } from "./python";
import {
  calls,
  commands,
  document,
  extensions,
  fire,
  settings,
  ui,
  Uri,
  workspace,
} from "./vscode";

/** A `sqlty` of `version` that finds no problems and lists one rule. */
function sqlty(version: string): string {
  const path = join(mkdtempSync(join(tmpdir(), "sqlty-extension-")), "sqlty");
  writeFileSync(
    path,
    `#!/bin/sh\ncase "$*" in\n*--version*) echo "sqlty ${version}" ;;\n*rule*) echo "select-star  warn  d" ;;\n*) echo "[]" ;;\nesac\n`,
  );
  chmodSync(path, 0o755);
  return path;
}

const context = {
  subscriptions: [] as { dispose(): unknown }[],
  extensionUri: Uri.file(resolve(".")),
};

/** The messages shown and the debug information of the commands run, in order. */
async function run(...names: string[]): Promise<unknown[]> {
  calls.length = 0;
  for (const name of names) {
    await commands.executeCommand(name);
  }
  return calls
    .filter(([name]) =>
      ["showErrorMessage", "showInformationMessage", "clipboard.writeText"].includes(name),
    )
    .map(([, text]) => text);
}

/** Waits for the restart a listener started without waiting, until a call named `name`. */
async function restarted(name: string): Promise<void> {
  while (!calls.some(([called]) => called === name)) {
    await setTimeout(5);
  }
}

const path = process.env.PATH;

beforeEach(() => {
  workspace.workspaceFolders = [
    { uri: Uri.file(mkdtempSync(join(tmpdir(), "sqlty-"))), name: "w" },
  ];
  ui.quickPick = () => undefined;
});

afterEach(async () => {
  await deactivate();
  context.subscriptions.splice(0).forEach((disposable) => disposable.dispose());
  settings.clear();
  workspace.isTrusted = true;
  process.env.PATH = path;
  server.starting = Promise.resolve();
  server.fails = false;
});

test("Restricted Mode checks with the engine until the workspace is trusted", async () => {
  workspace.isTrusted = false;
  await activate(context as never);
  assert.deepEqual(await run("sqlty.checkWorkspace", "sqlty.fixAll", "sqlty.debugInformation"), [
    "sqlty: sqlty server is not running; see the logs.",
    "sqlty extension: unknown\nFake Code: 1.0.0 (desktop)\nWorkspace trusted: false\nChecks with: the bundled engine (Restricted Mode)",
    "sqlty: debug information copied to the clipboard.",
  ]);

  const found = sqlty("0.5.0");
  settings.set("sqlty.path", [found]);
  server.capabilities = {
    completionProvider: {},
    codeActionProvider: { codeActionKinds: ["source.fixAll.sqlty"] },
  };
  workspace.isTrusted = true;
  calls.length = 0;
  await fire("trust");
  await restarted("client.start");
  assert.deepEqual(
    calls.find(([name]) => name === "client.start"),
    ["client.start", found, "server"],
  );
  assert.deepEqual(await run("sqlty.fixAll", "sqlty.format", "sqlty.checkWorkspace"), [
    "sqlty: source.format.sqlty needs sqlty 0.6.0 or newer, found 0.5.0. Upgrade it: `uv add --dev 'sqlty>=0.6.0'` or `pip install -U sqlty`.",
    "sqlty: no problems outside the open files.",
  ]);
  const [info] = await run("sqlty.debugInformation");
  assert.equal(
    String(info).split("\n").slice(3).join("\n"),
    [
      "Checks with: sqlty server 0.5.0",
      `Command: ${found} server`,
      "Found in: sqlty.path",
      `sqlty.path: ${JSON.stringify([found])}`,
      "sqlty.interpreter: []",
      'sqlty.breaking: {"against":"","direction":"both"}',
      "sqlty.completion.enable: true",
      "Code actions: source.fixAll.sqlty",
      `Workspace folders: ${workspace.workspaceFolders![0].uri.fsPath}`,
    ].join("\n"),
  );
  const offered: string[] = [];
  ui.quickPick = (items) => {
    offered.push(...(items as { label: string }[]).map((item) => item.label));
    return undefined;
  };
  await run("sqlty.explainRule");
  assert.deepEqual(offered, ["select-star", "All rules"]);
});

test("settings, folders and the interpreter restart the server", async () => {
  process.env.PATH = "";
  const getExtension = extensions.getExtension;
  extensions.getExtension = (id) =>
    id === "ms-python.python" ? { packageJSON: { version: "1" } } : undefined;
  python.selected = undefined;
  server.capabilities = {};
  await activate(context as never);
  const missing =
    "sqlty: no sqlty found. Install sqlty in the selected interpreter, or set sqlty.path.";
  const [, , info] = await run("sqlty.checkWorkspace", "sqlty.fixAll", "sqlty.debugInformation");
  assert.deepEqual(
    calls.filter(([name]) => name === "showErrorMessage").map(([, text]) => text),
    [missing, missing],
  );
  assert.ok(String(info).endsWith(`Checks with: nothing (${missing})`), String(info));

  python.selected = sqlty("0.3.0");
  calls.length = 0;
  await fire("python");
  await restarted("output.warn");
  assert.deepEqual(calls.find(([name]) => name === "client.start")?.slice(2), [
    "-m",
    "sqlty",
    "server",
  ]);
  assert.match(
    String(calls.find(([name]) => name === "output.warn")?.[1]),
    /completion needs sqlty 0\.4\.0/,
  );
  assert.match(String((await run("sqlty.fixAll"))[0]), /source\.fixAll\.sqlty needs sqlty 0\.6\.0/);
  assert.match(
    String((await run("sqlty.debugInformation"))[0]),
    /\nCode actions: \nWorkspace folders: /,
  );

  calls.length = 0;
  await fire("configuration", { affectsConfiguration: () => false });
  assert.deepEqual(calls, []);
  settings.set("sqlty.interpreter", [python.selected]);
  await fire("configuration", { affectsConfiguration: (section: string) => section === "sqlty" });
  await restarted("client.start");
  calls.length = 0;
  await fire("folders");
  await restarted("client.start");
  workspace.workspaceFolders = undefined;
  extensions.getExtension = getExtension;
});

/** The calls to the client and its output, by name. */
function lifecycle(): string[] {
  return calls
    .map(([name]) => name)
    .filter((name) => /^client\.|^output\.(warn|error)$/.test(name));
}

test("a change while the server starts restarts it once, after it has started", async () => {
  settings.set("sqlty.path", [sqlty("0.6.0")]);
  server.capabilities = { completionProvider: {} };
  let ready = () => {};
  server.starting = new Promise((resolve) => (ready = resolve));
  calls.length = 0;
  const activating = activate(context as never);
  await restarted("client.start");
  await fire("folders");
  await fire("python");
  ready();
  await activating;
  while (lifecycle().length < 3) {
    await setTimeout(5);
  }
  await setTimeout(20);
  assert.deepEqual(lifecycle(), ["client.start", "client.dispose", "client.start"]);
});

test("a server that did not start is replaced", async () => {
  settings.set("sqlty.path", [sqlty("0.6.0")]);
  server.capabilities = { completionProvider: {} };
  server.fails = true;
  await assert.rejects(activate(context as never), /Server initialization failed/);
  server.fails = false;
  calls.length = 0;
  await fire("folders");
  await restarted("client.start");
  assert.deepEqual(
    calls.filter(([name]) => name === "output.error").map(([, text]) => text),
    [
      "Stopping sqlty server failed: Error: Client is not running and can't be stopped. It's current state is: failed",
    ],
  );
});

test("opening a file drops its workspace check results", async () => {
  settings.set("sqlty.path", [sqlty("0.6.0")]);
  await activate(context as never);
  const opened = document("/w/a.py", "");
  const [collection] = context.subscriptions.filter(
    (disposable) => "entries" in disposable,
  ) as never as {
    entries: Map<string, unknown>;
    set(uri: Uri, found: unknown[]): void;
  }[];
  collection.set(opened.uri, []);
  await fire("open", opened);
  assert.equal(collection.entries.size, 0);
});
