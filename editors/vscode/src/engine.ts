import * as vscode from "vscode";

import { type Engine, type Project, project } from "./web/config";

const SELECTOR: vscode.DocumentSelector = { language: "python" };

/**
 * The engine compiled to WebAssembly, run in the extension host instead of `sqlty server`:
 * diagnostics, completion and hover, as the docs playground has them. It runs no process and
 * no code of the workspace, so VS Code for the Web and Restricted Mode get it.
 */
export async function startEngine(
  extension: vscode.Uri,
  output: vscode.LogOutputChannel,
): Promise<vscode.Disposable> {
  const diagnostics = vscode.languages.createDiagnosticCollection("sqlty");
  const fixes = new WeakMap<vscode.Diagnostic, Edit[]>();
  const wasm = vscode.Uri.joinPath(extension, "dist", "sqlty_wasm.wasm");
  const engine = await load(await vscode.workspace.fs.readFile(wasm));

  const projects = new Map<string, Promise<Project>>();
  const projectOf = (document: vscode.TextDocument): Promise<Project> => {
    const directory = vscode.Uri.joinPath(document.uri, "..");
    const key = directory.toString();
    if (!projects.has(key)) {
      projects.set(key, project(engine, directory));
    }
    return projects.get(key)!;
  };
  const ask = async (document: vscode.TextDocument, request: Record<string, unknown>) => {
    const found = await projectOf(document);
    if ("error" in found) {
      output.error(found.error);
      return undefined;
    }
    const answer = engine({
      ...request,
      source: document.getText(),
      schema: found.schema,
      dialect: "postgres",
      settings: found.settings,
    });
    if (answer !== null && "error" in answer) {
      output.error(answer.error);
      return undefined;
    }
    return answer;
  };

  const check = async (document: vscode.TextDocument) => {
    if (vscode.languages.match(SELECTOR, document) === 0) {
      return;
    }
    const answer = await ask(document, { op: "analyze" });
    diagnostics.set(
      document.uri,
      (answer?.diagnostics ?? []).map((found: Found) => {
        const created = diagnostic(document, found);
        fixes.set(created, found.fix);
        return created;
      }),
    );
  };
  const checkAll = () => vscode.workspace.textDocuments.forEach((document) => void check(document));

  const watcher = vscode.workspace.createFileSystemWatcher("**/*.{toml,sql}");
  const reload = () => {
    projects.clear();
    checkAll();
  };
  const subscriptions = [
    diagnostics,
    watcher,
    watcher.onDidChange(reload),
    watcher.onDidCreate(reload),
    watcher.onDidDelete(reload),
    vscode.workspace.onDidOpenTextDocument((document) => void check(document)),
    vscode.workspace.onDidChangeTextDocument((event) => void check(event.document)),
    vscode.workspace.onDidCloseTextDocument((document) => diagnostics.delete(document.uri)),
    vscode.languages.registerCompletionItemProvider(
      SELECTOR,
      {
        async provideCompletionItems(document, position) {
          const offset = document.offsetAt(position);
          const answer = await ask(document, { op: "complete", offset });
          if (answer == null) {
            return undefined;
          }
          const range = new vscode.Range(
            document.positionAt(answer.start),
            document.positionAt(answer.end),
          );
          return answer.items.map((item: Candidate, index: number) => {
            const completion = new vscode.CompletionItem(item.label, KINDS[item.kind]);
            completion.detail = item.detail;
            completion.range = range;
            completion.sortText = String(index).padStart(4, "0");
            return completion;
          });
        },
      },
      " ",
      ".",
      ",",
    ),
    vscode.languages.registerCodeActionsProvider(
      SELECTOR,
      {
        provideCodeActions(document, _range, context) {
          return context.diagnostics.flatMap((found) => {
            const edits = fixes.get(found) ?? [];
            if (edits.length === 0) {
              return [];
            }
            const action = new vscode.CodeAction(
              `Fix ${(found.code as { value: string }).value}`,
              vscode.CodeActionKind.QuickFix,
            );
            action.diagnostics = [found];
            action.edit = new vscode.WorkspaceEdit();
            for (const edit of edits) {
              const range = new vscode.Range(
                document.positionAt(edit.start),
                document.positionAt(edit.end),
              );
              action.edit.replace(document.uri, range, edit.replacement);
            }
            return [action];
          });
        },
      },
      { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix] },
    ),
    vscode.languages.registerHoverProvider(SELECTOR, {
      async provideHover(document, position) {
        const answer = await ask(document, { op: "hover", offset: document.offsetAt(position) });
        if (answer == null) {
          return undefined;
        }
        return new vscode.Hover(
          new vscode.MarkdownString(answer.contents),
          new vscode.Range(document.positionAt(answer.start), document.positionAt(answer.end)),
        );
      },
    }),
  ];
  checkAll();
  return vscode.Disposable.from(...subscriptions);
}

type Edit = { start: number; end: number; replacement: string };
type Found = {
  rule: string;
  message: string;
  severity: string;
  start: number;
  end: number;
  fix: Edit[];
};
type Candidate = { label: string; kind: string; detail: string };

const KINDS: Record<string, vscode.CompletionItemKind> = {
  table: vscode.CompletionItemKind.Class,
  view: vscode.CompletionItemKind.Interface,
  cte: vscode.CompletionItemKind.Variable,
  column: vscode.CompletionItemKind.Field,
  join: vscode.CompletionItemKind.Snippet,
};

function diagnostic(document: vscode.TextDocument, found: Found): vscode.Diagnostic {
  const range = new vscode.Range(document.positionAt(found.start), document.positionAt(found.end));
  const severity =
    found.severity === "error"
      ? vscode.DiagnosticSeverity.Error
      : vscode.DiagnosticSeverity.Warning;
  const diagnostic = new vscode.Diagnostic(range, found.message, severity);
  diagnostic.source = "sqlty";
  diagnostic.code = {
    value: found.rule,
    target: vscode.Uri.parse(`https://sqlty.kuncar.dev/reference/rules/#${found.rule}`),
  };
  return diagnostic;
}

/** The engine of `rust/crates/sqlty-wasm`, as `docs/assets/playground/engine.js` loads it. */
async function load(bytes: Uint8Array): Promise<Engine> {
  const { instance } = await WebAssembly.instantiate(bytes as BufferSource);
  const { memory, buffer, run, output } = instance.exports as {
    memory: WebAssembly.Memory;
    buffer: (length: number) => number;
    run: () => number;
    output: () => number;
  };
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  return (request) => {
    const encoded = encoder.encode(JSON.stringify(request));
    const at = buffer(encoded.length);
    new Uint8Array(memory.buffer, at, encoded.length).set(encoded);
    const length = run();
    return JSON.parse(decoder.decode(new Uint8Array(memory.buffer, output(), length)));
  };
}
