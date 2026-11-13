import { PythonExtension } from "@vscode/python-extension";
import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
} from "vscode-languageclient/node";

import { registerCommands } from "./commands";
import { loadEngine, startEngine } from "./engine";
import {
  completionNotice,
  findServer,
  type Server,
  type Settings,
  SOURCE_ACTIONS_VERSION,
} from "./server";
import type { Engine } from "./web/config";
import { breakingAgainst, checkWorkspace, type Cli, rules } from "./workspace";

let client: LanguageClient | undefined;
let engine: vscode.Disposable | undefined;
let extension: vscode.Uri;
let output: vscode.LogOutputChannel | undefined;
let running: Server | undefined;
let problem: string | undefined;
let wasm: Promise<Engine> | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  extension = context.extensionUri;
  output = vscode.window.createOutputChannel("sqlty", { log: true });
  const workspace = vscode.languages.createDiagnosticCollection("sqlty-workspace");
  context.subscriptions.push(
    output,
    workspace,
    vscode.commands.registerCommand("sqlty.restart", restart),
    vscode.commands.registerCommand("sqlty.checkWorkspace", () =>
      running ? checkWorkspace(cli(running), workspace, output!) : notRunning(),
    ),
    vscode.commands.registerCommand("sqlty.breakingAgainst", breakingAgainst),
    vscode.workspace.onDidOpenTextDocument((document) => workspace.delete(document.uri)),
    registerCommands({
      output,
      engine: () => (wasm ??= loadEngine(extension)),
      unsupported,
      describe,
      rules: async () => (running && vscode.workspace.isTrusted ? rules(cli(running)) : undefined),
    }),
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
    problem = undefined;
    output?.info("Restricted Mode: the bundled engine checks until the workspace is trusted");
    return;
  }
  const settings = readSettings();
  const interpreter = settings.interpreter.length > 0 ? settings.interpreter : await selected();
  const found = await findServer(settings, interpreter);
  if ("error" in found) {
    problem = found.error;
    output?.error(found.error);
    void vscode.window.showErrorMessage(found.error);
    return;
  }
  problem = undefined;
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
  running = server;
  void vscode.commands.executeCommand("setContext", "sqlty.server", true);
  const advertised = client.initializeResult?.capabilities.completionProvider !== undefined;
  const notice = completionNotice(settings, advertised, server.version);
  if (notice !== undefined) {
    output?.warn(notice);
  }
}

async function stop(): Promise<void> {
  running = undefined;
  void vscode.commands.executeCommand("setContext", "sqlty.server", false);
  engine?.dispose();
  engine = undefined;
  const stopping = client;
  client = undefined;
  await stopping?.dispose();
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

/** `sqlty` without the `server` subcommand. */
function cli(server: Server): Cli {
  return { command: server.command, args: server.args.slice(0, -1) };
}

function notRunning(): void {
  void vscode.window.showErrorMessage(
    problem ?? "sqlty: sqlty server is not running; see the logs.",
  );
}

function unsupported(kind: string): string | undefined {
  if (engine !== undefined) {
    return undefined;
  }
  if (client === undefined || running === undefined) {
    return problem ?? "sqlty: sqlty server is not running; see the logs.";
  }
  const provider = client.initializeResult?.capabilities.codeActionProvider;
  const kinds = typeof provider === "object" ? (provider.codeActionKinds ?? []) : [];
  if (kinds.includes(kind)) {
    return undefined;
  }
  return `sqlty: ${kind} needs sqlty ${SOURCE_ACTIONS_VERSION} or newer, found ${running.version}. Upgrade it: \`uv add --dev 'sqlty>=${SOURCE_ACTIONS_VERSION}'\` or \`pip install -U sqlty\`.`;
}

function describe(): string[] {
  if (engine !== undefined) {
    return ["Checks with: the bundled engine (Restricted Mode)"];
  }
  if (running === undefined) {
    return [`Checks with: nothing (${problem ?? "sqlty server is not running"})`];
  }
  const settings = readSettings();
  const provider = client?.initializeResult?.capabilities.codeActionProvider;
  const kinds = typeof provider === "object" ? (provider.codeActionKinds ?? []) : [];
  return [
    `Checks with: sqlty server ${running.version}`,
    `Command: ${[running.command, ...running.args].join(" ")}`,
    `Found in: ${running.origin}`,
    `sqlty.path: ${JSON.stringify(settings.path)}`,
    `sqlty.interpreter: ${JSON.stringify(settings.interpreter)}`,
    `sqlty.breaking: ${JSON.stringify(settings.breaking)}`,
    `sqlty.completion.enable: ${settings.completion.enable}`,
    `Code actions: ${kinds.join(", ")}`,
    `Workspace folders: ${(vscode.workspace.workspaceFolders ?? []).map((folder) => folder.uri.fsPath).join(", ")}`,
  ];
}
