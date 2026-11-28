// The part of the `vscode` API the extension uses, for unit tests under plain node: files are
// real, the UI is whatever a test sets on `ui`, and every call it makes is in `calls`.
import { readdirSync, readFileSync, statSync, globSync } from "node:fs";
import { join, relative } from "node:path";

import type * as vscode from "vscode";
import { URI, Utils } from "vscode-uri";

export const calls: [string, ...unknown[]][] = [];

/** What the user answers: each prompt returns what its function does. */
export const ui = {
  quickPick: (items: unknown[]): unknown => items[0],
  inputBox: (): string | undefined => undefined,
  openDialog: (): Uri[] | undefined => undefined,
  /** `"kill"` also stops a running process. */
  cancelled: false as boolean | "kill",
};

export class Uri {
  private constructor(private readonly uri: URI) {}
  static file(path: string): Uri {
    return new Uri(URI.file(path));
  }
  static parse(text: string): Uri {
    return new Uri(URI.parse(text));
  }
  static joinPath(base: Uri, ...paths: string[]): Uri {
    return new Uri(Utils.joinPath(base.uri, ...paths));
  }
  with(change: { path?: string }): Uri {
    return new Uri(this.uri.with(change));
  }
  get path(): string {
    return this.uri.path;
  }
  get fsPath(): string {
    return this.uri.fsPath;
  }
  get scheme(): string {
    return this.uri.scheme;
  }
  toString(): string {
    return this.uri.toString();
  }
}

export class Position {
  constructor(
    readonly line: number,
    readonly character: number,
  ) {}
  isBefore(other: Position): boolean {
    return this.line < other.line || (this.line === other.line && this.character < other.character);
  }
}

export class Range {
  readonly start: Position;
  readonly end: Position;
  constructor(start: Position | number, end: Position | number, line?: number, column?: number) {
    this.start = typeof start === "number" ? new Position(start, end as number) : start;
    this.end = typeof start === "number" ? new Position(line!, column!) : (end as Position);
  }
  contains(at: Position): boolean {
    return !at.isBefore(this.start) && !this.end.isBefore(at);
  }
}

export enum DiagnosticSeverity {
  Error = 0,
  Warning = 1,
}

export class Diagnostic {
  source?: string;
  code?: string | number | { value: string | number; target: Uri };
  constructor(
    readonly range: Range,
    readonly message: string,
    readonly severity: DiagnosticSeverity,
  ) {}
}

export class RelativePattern {
  constructor(
    readonly base: Uri,
    readonly pattern: string,
  ) {}
}

export enum FileType {
  File = 1,
  Directory = 2,
}

export enum ProgressLocation {
  Notification = 15,
}

export enum QuickPickItemKind {
  Separator = -1,
}

export enum ConfigurationTarget {
  Workspace = 2,
}

export enum UIKind {
  Desktop = 1,
  Web = 2,
}

export class Disposable {
  constructor(private readonly dispose_: () => void) {}
  static from(...disposables: { dispose(): unknown }[]): Disposable {
    return new Disposable(() => disposables.forEach((disposable) => disposable.dispose()));
  }
  dispose(): void {
    this.dispose_();
  }
}

export enum CompletionItemKind {
  Function = 2,
  Field = 4,
  Variable = 5,
  Class = 6,
  Interface = 7,
  Snippet = 14,
}

export class CompletionItem {
  detail?: string;
  insertText?: string;
  range?: Range;
  sortText?: string;
  constructor(
    readonly label: string,
    readonly kind?: CompletionItemKind,
  ) {}
}

export class CodeActionKind {
  static readonly Empty = new CodeActionKind("");
  static readonly QuickFix = new CodeActionKind("quickfix");
  private constructor(readonly value: string) {}
  append(part: string): CodeActionKind {
    return new CodeActionKind(this.value ? `${this.value}.${part}` : part);
  }
  contains(other: CodeActionKind): boolean {
    return (
      this.value === "" || other.value === this.value || other.value.startsWith(`${this.value}.`)
    );
  }
}

export class CodeAction {
  diagnostics?: Diagnostic[];
  edit?: WorkspaceEdit;
  constructor(
    readonly title: string,
    readonly kind?: CodeActionKind,
  ) {}
}

export class WorkspaceEdit {
  readonly replaced: [string, Range, string][] = [];
  replace(uri: Uri, range: Range, text: string): void {
    this.replaced.push([uri.toString(), range, text]);
  }
}

export class MarkdownString {
  constructor(readonly value: string) {}
}

export class Hover {
  constructor(
    readonly contents: MarkdownString,
    readonly range: Range,
  ) {}
}

export class ParameterInformation {
  constructor(readonly label: [number, number]) {}
}

export class SignatureInformation {
  parameters: ParameterInformation[] = [];
  constructor(
    readonly label: string,
    readonly documentation: MarkdownString,
  ) {}
}

export class SignatureHelp {
  signatures: SignatureInformation[] = [];
  activeSignature = 0;
  activeParameter = 0;
}

/** A Python file holding `text`, as the editor has it open. */
export function document(path: string, text: string, languageId = "python") {
  const lines = text.split("\n");
  return {
    uri: Uri.file(path),
    languageId,
    lineCount: lines.length,
    getText: () => text,
    offsetAt: (at: Position) =>
      lines.slice(0, at.line).reduce((sum, line) => sum + line.length + 1, 0) + at.character,
    positionAt: (offset: number) => {
      const before = text.slice(0, offset).split("\n");
      return new Position(before.length - 1, before.at(-1)!.length);
    },
  };
}

class Collection {
  readonly entries = new Map<string, Diagnostic[]>();
  set(uri: Uri, diagnostics: Diagnostic[]): void {
    this.entries.set(uri.toString(), diagnostics);
  }
  delete(uri: Uri): void {
    this.entries.delete(uri.toString());
  }
  clear(): void {
    this.entries.clear();
  }
  dispose(): void {}
}

export function diagnostics(): Collection & vscode.DiagnosticCollection {
  return new Collection() as unknown as Collection & vscode.DiagnosticCollection;
}

export type CancellationToken = {
  isCancellationRequested: boolean;
  onCancellationRequested: (listener: () => void) => void;
};
export function output(): vscode.LogOutputChannel {
  const channel = {
    error: (message: string) => calls.push(["output.error", message]),
    info: (message: string) => calls.push(["output.info", message]),
    warn: (message: string) => calls.push(["output.warn", message]),
    show: () => calls.push(["output.show"]),
    dispose: () => undefined,
  };
  return channel as unknown as vscode.LogOutputChannel;
}

type Listener = (...args: any[]) => unknown;

const listeners = new Map<string, Set<Listener>>();

/** Calls the listeners of the event `name` with `args`. */
export async function fire(name: string, ...args: unknown[]): Promise<void> {
  await Promise.all([...(listeners.get(name) ?? [])].map((listener) => listener(...args)));
}

export const event = (name: string) => (listener: Listener) => {
  const set = listeners.get(name) ?? new Set();
  listeners.set(name, set.add(listener));
  return new Disposable(() => set.delete(listener));
};

/** The providers registered, by kind: completion, codeActions (in order), hover and signature. */
export const providers: Record<string, any[]> = {
  completion: [],
  codeActions: [],
  hover: [],
  signature: [],
};

const provide = (kind: string) => (_selector: unknown, provider: unknown) => {
  providers[kind].push(provider);
  return new Disposable(() => providers[kind].splice(providers[kind].indexOf(provider), 1));
};

/** The settings, by `section.key`. */
export const settings = new Map<string, unknown>();
const commands_ = new Map<string, (...args: unknown[]) => unknown>();

export const workspace = {
  workspaceFolders: undefined as { uri: Uri; name: string }[] | undefined,
  textDocuments: [] as { uri: Uri }[],
  isTrusted: true,
  fs: {
    readFile: async (uri: Uri) => new Uint8Array(readFileSync(uri.fsPath)),
    stat: async (uri: Uri) => ({
      type: statSync(uri.fsPath).isDirectory() ? FileType.Directory : FileType.File,
    }),
    readDirectory: async (uri: Uri) =>
      readdirSync(uri.fsPath, { withFileTypes: true }).map(
        (entry) => [entry.name, entry.isDirectory() ? FileType.Directory : FileType.File] as const,
      ),
  },
  findFiles: async (pattern: RelativePattern) =>
    globSync(pattern.pattern, { cwd: pattern.base.fsPath }).map((path) =>
      Uri.file(join(pattern.base.fsPath, path)),
    ),
  getConfiguration: (section: string) => ({
    get: <T>(key: string, fallback: T): T =>
      (settings.get(`${section}.${key}`) as T | undefined) ?? fallback,
    update: async (key: string, value: unknown, target: ConfigurationTarget) => {
      calls.push(["configuration.update", `${section}.${key}`, value, target]);
      settings.set(`${section}.${key}`, value);
    },
  }),
  applyEdit: async (edit: unknown) => calls.push(["workspace.applyEdit", edit]),
  createFileSystemWatcher: (pattern: string) => ({
    onDidChange: event(`watcher.change ${pattern}`),
    onDidCreate: event(`watcher.create ${pattern}`),
    onDidDelete: event(`watcher.delete ${pattern}`),
    dispose: () => undefined,
  }),
  onDidOpenTextDocument: event("open"),
  onDidChangeTextDocument: event("change"),
  onDidCloseTextDocument: event("close"),
  onDidChangeConfiguration: event("configuration"),
  onDidChangeWorkspaceFolders: event("folders"),
  onDidGrantWorkspaceTrust: event("trust"),
};

export const window = {
  activeTextEditor: undefined as unknown,
  showErrorMessage: async (message: string) => calls.push(["showErrorMessage", message]),
  showInformationMessage: async (message: string) =>
    calls.push(["showInformationMessage", message]),
  showQuickPick: async (items: unknown[]) => ui.quickPick(items),
  showInputBox: async () => ui.inputBox(),
  createOutputChannel: () => output(),
  showOpenDialog: async () => ui.openDialog(),
  withProgress: async <T>(
    _options: unknown,
    task: (progress: unknown, token: CancellationToken) => Promise<T>,
  ): Promise<T> => {
    const token = {
      get isCancellationRequested() {
        return ui.cancelled !== false;
      },
      onCancellationRequested: (listener: () => void) => {
        if (ui.cancelled === "kill") {
          listener();
        }
      },
    };
    return task(undefined, token);
  },
};

export const commands = {
  registerCommand: (name: string, callback: (...args: unknown[]) => unknown) => {
    commands_.set(name, callback);
    return new Disposable(() => commands_.delete(name));
  },
  executeCommand: async (name: string, ...args: unknown[]) => {
    calls.push(["executeCommand", name, ...args]);
    return commands_.get(name)?.(...args);
  },
};

export const languages = {
  diagnostics: [] as [Uri, Diagnostic[]][],
  collections: [] as Collection[],
  createDiagnosticCollection(): Collection {
    const created = new Collection();
    this.collections.push(created);
    return created;
  },
  match: (_selector: unknown, document: { languageId: string }) =>
    document.languageId === "python" ? 10 : 0,
  registerCompletionItemProvider: provide("completion"),
  registerCodeActionsProvider: provide("codeActions"),
  registerHoverProvider: provide("hover"),
  registerSignatureHelpProvider: provide("signature"),
  getDiagnostics(uri?: Uri): unknown {
    return uri === undefined
      ? this.diagnostics
      : (this.diagnostics.find(([at]) => at.toString() === uri.toString())?.[1] ?? []);
  },
};

export const env = {
  appName: "Fake Code",
  uiKind: UIKind.Desktop,
  clipboard: { writeText: async (text: string) => calls.push(["clipboard.writeText", text]) },
  openExternal: async (uri: Uri) => calls.push(["openExternal", uri.toString()]),
};

export const extensions = {
  getExtension: (_id: string): { packageJSON: { version: string } } | undefined => undefined,
};

export const version = "1.0.0";

/** The path of `uri` from `base`, as a test compares them. */
export function under(base: string, uri: Uri): string {
  return relative(base, uri.fsPath);
}
