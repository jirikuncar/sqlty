import * as vscode from "vscode";

/** VS Code for the Web has no processes, so it gets the SQL highlighting but no `sqlty server`. */
export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel("sqlty", { log: true });
  context.subscriptions.push(output);
  output.info("VS Code for the Web: SQL highlighting only, sqlty server needs the desktop app");
}
