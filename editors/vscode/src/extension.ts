import { PythonExtension } from "@vscode/python-extension";
import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
} from "vscode-languageclient/node";

import { startEngine } from "./engine";
import { completionNotice, findServer, type Settings } from "./server";

let client: LanguageClient | undefined;
let engine: vscode.Disposable | undefined;
let extension: vscode.Uri;
let output: vscode.LogOutputChannel | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  extension = context.extensionUri;
  output = vscode.window.createOutputChannel("sqlty", { log: true });
  context.subscriptions.push(
    output,
    vscode.commands.registerCommand("sqlty.restart", restart),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("sqlty")) {
        void restart();
      }
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(() => void restart()),
    vscode.workspace.onDidGrantWorkspaceTrust(() => void restart()),
  );
  const python = await pythonExtension();
  if (python !== undefined) {
    context.subscriptions.push(
      python.environments.onDidChangeActiveEnvironmentPath(() => void restart()),
    );
  }
  await start();
}

export async function deactivate(): Promise<void> {
  await stop();
}

async function restart(): Promise<void> {
  await stop();
  await start();
}

async function start(): Promise<void> {
  if (!vscode.workspace.isTrusted) {
    engine = await startEngine(extension, output!);
    output?.info("Restricted Mode: the bundled engine checks until the workspace is trusted");
    return;
  }
  const settings = readSettings();
  const interpreter = settings.interpreter.length > 0 ? settings.interpreter : await selected();
  const found = await findServer(settings, interpreter);
  if ("error" in found) {
    output?.error(found.error);
    void vscode.window.showErrorMessage(found.error);
    return;
  }
  const { server } = found;
  const command = [server.command, ...server.args].join(" ");
  output?.info(`Starting ${command} (sqlty ${server.version}, from ${server.origin})`);
  const serverOptions: ServerOptions = { command: server.command, args: server.args };
  const clientOptions: LanguageClientOptions = {
    // A `.sql` file gets diagnostics when the `queries` key names it.
    documentSelector: [
      { scheme: "file", language: "python" },
      { scheme: "file", language: "sql" },
    ],
    initializationOptions: {
      breaking: settings.breaking,
      completion: settings.completion,
      // VS Code shows one semantic-token provider per file: sqlty's would replace Pylance's.
      highlighting: { enable: false },
    },
    outputChannel: output,
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher(
        "**/{pyproject.toml,sqlty.toml,Cargo.toml,*.sql}",
      ),
    },
  };
  client = new LanguageClient("sqlty", "sqlty", serverOptions, clientOptions);
  await client.start();
  const advertised = client.initializeResult?.capabilities.completionProvider !== undefined;
  const notice = completionNotice(settings, advertised, server.version);
  if (notice !== undefined) {
    output?.warn(notice);
  }
}

async function stop(): Promise<void> {
  engine?.dispose();
  engine = undefined;
  const running = client;
  client = undefined;
  await running?.dispose();
}

function readSettings(): Settings {
  const config = vscode.workspace.getConfiguration("sqlty", folder());
  return {
    path: config.get<string[]>("path", []),
    interpreter: config.get<string[]>("interpreter", []),
    breaking: {
      against: config.get<string>("breaking.against", ""),
      direction: config.get<Settings["breaking"]["direction"]>("breaking.direction", "both"),
    },
    completion: { enable: config.get<boolean>("completion.enable", true) },
  };
}

/** The interpreter selected in the Python extension, if it is installed. */
async function selected(): Promise<string[] | undefined> {
  const python = await pythonExtension();
  if (python === undefined) {
    return undefined;
  }
  const active = python.environments.getActiveEnvironmentPath(folder());
  const environment = await python.environments.resolveEnvironment(active);
  const executable = environment?.executable.uri?.fsPath;
  return executable === undefined ? undefined : [executable];
}

async function pythonExtension(): Promise<PythonExtension | undefined> {
  if (vscode.extensions.getExtension("ms-python.python") === undefined) {
    return undefined;
  }
  return PythonExtension.api();
}

/** Settings and the interpreter are those of the first folder: one server serves them all. */
function folder(): vscode.Uri | undefined {
  return vscode.workspace.workspaceFolders?.[0]?.uri;
}
