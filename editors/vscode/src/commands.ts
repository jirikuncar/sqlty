import * as vscode from "vscode";

import { encode, MAX_LINK, PLAYGROUND, session } from "./playground";
import { type Engine, project } from "./web/config";

export const RULES = "https://sqlty.kuncar.dev/reference/rules/";

/** The whole-file code actions of the server and the bundled engine, by command. */
export const SOURCE_ACTIONS = {
  "sqlty.fixAll": "source.fixAll.sqlty",
  "sqlty.format": "source.format.sqlty",
  "sqlty.rewrite": "source.rewrite.sqlty",
  "sqlty.strip": "source.strip.sqlty",
} as const;

export type Rule = { name: string; severity: string; description: string };

/** What the commands read of the extension where they run: desktop, Restricted Mode or web. */
export type Host = {
  output: vscode.LogOutputChannel;
  engine: () => Promise<Engine>;
  /** Why `kind` cannot run, when the running server does not offer it. */
  unsupported: (kind: string) => string | undefined;
  /** Lines for Show Debug Information. */
  describe: () => string[];
  /** The rules `sqlty rule` lists, when there is a `sqlty` to ask. */
  rules: () => Promise<Rule[] | undefined>;
};

export function registerCommands(host: Host): vscode.Disposable {
  return vscode.Disposable.from(
    ...Object.entries(SOURCE_ACTIONS).map(([command, kind]) =>
      vscode.commands.registerCommand(command, () => sourceAction(host, kind)),
    ),
    vscode.commands.registerCommand("sqlty.showLogs", () => host.output.show()),
    vscode.commands.registerCommand("sqlty.debugInformation", () => debugInformation(host)),
    vscode.commands.registerCommand("sqlty.explainRule", () => explainRule(host)),
    vscode.commands.registerCommand("sqlty.openInPlayground", () => openInPlayground(host)),
  );
}

async function sourceAction(host: Host, kind: string): Promise<void> {
  const reason = host.unsupported(kind);
  if (reason !== undefined) {
    void vscode.window.showErrorMessage(reason);
    return;
  }
  const document = vscode.window.activeTextEditor?.document;
  if (document === undefined) {
    return;
  }
  const whole = new vscode.Range(0, 0, document.lineCount, 0);
  const actions = await vscode.commands.executeCommand<vscode.CodeAction[]>(
    "vscode.executeCodeActionProvider",
    document.uri,
    whole,
    kind,
  );
  const action = actions.find((found) => found.kind?.value === kind && found.edit !== undefined);
  if (action?.edit === undefined) {
    void vscode.window.showInformationMessage(`sqlty: ${kind} changes nothing in this file.`);
    return;
  }
  await vscode.workspace.applyEdit(action.edit);
}

async function debugInformation(host: Host): Promise<void> {
  const extension = vscode.extensions.getExtension("jirikuncar.sqlty");
  const lines = [
    `sqlty extension: ${extension?.packageJSON.version ?? "unknown"}`,
    `${vscode.env.appName}: ${vscode.version} (${vscode.env.uiKind === vscode.UIKind.Web ? "web" : "desktop"})`,
    `Workspace trusted: ${vscode.workspace.isTrusted}`,
    ...host.describe(),
  ];
  const text = lines.join("\n");
  host.output.info(`Debug information:\n${text}`);
  host.output.show();
  await vscode.env.clipboard.writeText(text);
  void vscode.window.showInformationMessage("sqlty: debug information copied to the clipboard.");
}

async function explainRule(host: Host): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  const here = editor
    ? vscode.languages
        .getDiagnostics(editor.document.uri)
        .filter(
          (found) => found.source === "sqlty" && found.range.contains(editor.selection.active),
        )
        .map(ruleOf)
    : [];
  const rules: Rule[] =
    (await host.rules()) ??
    [
      ...new Set(
        vscode.languages
          .getDiagnostics()
          .flatMap(([, found]) => found.filter((d) => d.source === "sqlty").map(ruleOf)),
      ),
    ]
      .sort()
      .map((name) => ({ name, severity: "", description: "" }));
  const items = rules.map((rule) => ({
    label: rule.name,
    description: rule.severity,
    detail: rule.description || undefined,
    picked: here.includes(rule.name),
  }));
  items.sort((a, b) => Number(b.picked) - Number(a.picked));
  const all = { label: "All rules", description: "", detail: RULES, picked: false };
  const picked = await vscode.window.showQuickPick([...items, all], {
    title: "sqlty: Explain Rule",
    placeHolder: "A rule, to open its page",
    matchOnDetail: true,
  });
  if (picked !== undefined) {
    await vscode.env.openExternal(
      vscode.Uri.parse(picked === all ? RULES : `${RULES}#${picked.label}`),
    );
  }
}

function ruleOf(diagnostic: vscode.Diagnostic): string {
  const code = diagnostic.code;
  return typeof code === "object" ? String(code.value) : String(code);
}

async function openInPlayground(host: Host): Promise<void> {
  const document = vscode.window.activeTextEditor?.document;
  if (document === undefined) {
    return;
  }
  const engine = await host.engine();
  const found = await project(engine, vscode.Uri.joinPath(document.uri, ".."));
  if ("error" in found) {
    void vscode.window.showErrorMessage(`sqlty: ${found.error}`);
    return;
  }
  const link = `${PLAYGROUND}#${await encode(session(document.getText(), found))}`;
  if (link.length > MAX_LINK) {
    void vscode.window.showErrorMessage(
      `sqlty: the file and its schema are too large to share as a link (${link.length} characters, at most ${MAX_LINK}).`,
    );
    return;
  }
  await vscode.env.clipboard.writeText(link);
  await vscode.env.openExternal(vscode.Uri.parse(link));
}
