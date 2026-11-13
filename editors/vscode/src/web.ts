import * as vscode from "vscode";

import { registerCommands } from "./commands";
import { loadEngine, startEngine } from "./engine";

/** VS Code for the Web runs no processes, so the engine runs in the browser instead. */
export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const output = vscode.window.createOutputChannel("sqlty", { log: true });
  let wasm: ReturnType<typeof loadEngine> | undefined;
  context.subscriptions.push(
    output,
    await startEngine(context.extensionUri, output),
    registerCommands({
      output,
      engine: () => (wasm ??= loadEngine(context.extensionUri)),
      unsupported: () => undefined,
      describe: () => ["Checks with: the bundled engine (VS Code for the Web)"],
      rules: async () => undefined,
    }),
  );
  output.info("VS Code for the Web: the engine runs in the browser, without sqlty server");
}
