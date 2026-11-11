import * as vscode from "vscode";

import { startEngine } from "./engine";

/** VS Code for the Web runs no processes, so the engine runs in the browser instead. */
export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const output = vscode.window.createOutputChannel("sqlty", { log: true });
  context.subscriptions.push(output, await startEngine(context.extensionUri, output));
  output.info("VS Code for the Web: the engine runs in the browser, without sqlty server");
}
