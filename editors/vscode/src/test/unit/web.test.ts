import assert from "node:assert/strict";
import { resolve } from "node:path";
import { test } from "node:test";

import { activate } from "../../web";
import { calls, commands, document, Uri, window } from "./vscode";

test("VS Code for the Web checks and shares with the bundled engine", async () => {
  const context = {
    subscriptions: [] as { dispose(): unknown }[],
    extensionUri: Uri.file(resolve(".")),
  };
  await activate(context as never);
  window.activeTextEditor = { document: document(resolve("fixture/q.py"), "x = 1") };
  calls.length = 0;
  await commands.executeCommand("sqlty.debugInformation");
  await commands.executeCommand("sqlty.openInPlayground");
  assert.match(String(calls[1][1]), /\nChecks with: the bundled engine \(VS Code for the Web\)$/);
  assert.match(String(calls.at(-1)![1]), /^https:\/\/sqlty\.kuncar\.dev\/playground\/#/);
  context.subscriptions.forEach((disposable) => disposable.dispose());
  window.activeTextEditor = undefined;
});
