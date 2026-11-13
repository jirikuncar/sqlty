import { execFile } from "node:child_process";
import { isAbsolute, join, relative } from "node:path";

import * as vscode from "vscode";

import type { Rule } from "./commands";

/** A `sqlty` to run: `command` with `args` before the subcommand. */
export type Cli = { command: string; args: string[] };

type Output = { stdout: string; stderr: string; code: number };

function run(
  cli: Cli,
  args: string[],
  cwd?: string,
  token?: vscode.CancellationToken,
): Promise<Output> {
  return new Promise((resolve, reject) => {
    const child = execFile(
      cli.command,
      [...cli.args, ...args],
      { cwd, maxBuffer: 256 * 1024 * 1024 },
      (error, stdout, stderr) => {
        const code = typeof error?.code === "number" ? error.code : error ? -1 : 0;
        if (error && code === -1) {
          reject(error);
        } else {
          resolve({ stdout, stderr, code });
        }
      },
    );
    token?.onCancellationRequested(() => child.kill());
  });
}

/** The rules `sqlty rule` lists: `name  severity  description` a line. */
export async function rules(cli: Cli): Promise<Rule[] | undefined> {
  try {
    const { stdout, code } = await run(cli, ["rule"]);
    if (code !== 0) {
      return undefined;
    }
    return stdout
      .split("\n")
      .map((line) => /^(\S+)\s+(\S+)\s+(.*)$/.exec(line))
      .filter((match) => match !== null)
      .map(([, name, severity, description]) => ({ name, severity, description }));
  } catch {
    return undefined;
  }
}

/** A position of `sqlty check --output-format json`: 1-based line and column. */
type At = { line: number; column: number };

type Found = {
  file: string;
  line?: number;
  rule: string;
  severity: string;
  message: string;
  start?: At;
  end?: At;
};

/**
 * `sqlty check` of each workspace folder, into `collection`: what the server does not check,
 * as it checks open files only. A file opened later is the server's again.
 */
export async function checkWorkspace(
  cli: Cli,
  collection: vscode.DiagnosticCollection,
  output: vscode.LogOutputChannel,
): Promise<void> {
  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "sqlty: checking the workspace",
      cancellable: true,
    },
    async (_, token) => {
      collection.clear();
      const open = new Set(
        vscode.workspace.textDocuments.map((document) => document.uri.toString()),
      );
      let files = 0;
      let problems = 0;
      for (const folder of vscode.workspace.workspaceFolders ?? []) {
        const cwd = folder.uri.fsPath;
        let checked: Output;
        try {
          checked = await run(cli, ["check", "--output-format", "json"], cwd, token);
        } catch (error) {
          if (!token.isCancellationRequested) {
            output.error(`sqlty check in ${cwd}: ${String(error)}`);
          }
          return;
        }
        if (token.isCancellationRequested) {
          return;
        }
        const { stdout, stderr, code } = checked;
        let found: Found[];
        try {
          found = JSON.parse(stdout) as Found[];
        } catch {
          output.error(`sqlty check in ${cwd} exited with ${code}:\n${stderr}`);
          void vscode.window.showErrorMessage(
            `sqlty: checking ${folder.name} failed; see the logs.`,
          );
          continue;
        }
        const byFile = new Map<string, vscode.Diagnostic[]>();
        for (const item of found) {
          const uri = vscode.Uri.file(isAbsolute(item.file) ? item.file : join(cwd, item.file));
          if (open.has(uri.toString())) {
            continue;
          }
          const key = uri.toString();
          byFile.set(key, [...(byFile.get(key) ?? []), diagnostic(item)]);
        }
        for (const [key, diagnostics] of byFile) {
          collection.set(vscode.Uri.parse(key), diagnostics);
          problems += diagnostics.length;
        }
        files += byFile.size;
      }
      void vscode.window.showInformationMessage(
        problems === 0
          ? "sqlty: no problems outside the open files."
          : `sqlty: ${problems} problem(s) in ${files} file(s) outside the open files; see Problems.`,
      );
    },
  );
}

function diagnostic(found: Found): vscode.Diagnostic {
  const line = (found.start?.line ?? found.line ?? 1) - 1;
  const start = new vscode.Position(line, (found.start?.column ?? 1) - 1);
  const end = found.end ? new vscode.Position(found.end.line - 1, found.end.column - 1) : start;
  const severity =
    found.severity === "error"
      ? vscode.DiagnosticSeverity.Error
      : vscode.DiagnosticSeverity.Warning;
  const created = new vscode.Diagnostic(new vscode.Range(start, end), found.message, severity);
  created.source = "sqlty";
  created.code = {
    value: found.rule,
    target: vscode.Uri.parse(`https://sqlty.kuncar.dev/reference/rules/#${found.rule}`),
  };
  return created;
}

/** Sets `sqlty.breaking.against` from a git ref, a snapshot or schema file, or a typed revision. */
export async function breakingAgainst(): Promise<void> {
  const folder = vscode.workspace.workspaceFolders?.[0];
  if (folder === undefined) {
    return;
  }
  const config = vscode.workspace.getConfiguration("sqlty", folder.uri);
  const current = config.get<string>("breaking.against", "");
  const refs = await gitRefs(folder.uri.fsPath);
  type Item = vscode.QuickPickItem & { value?: string; action?: "browse" | "type" };
  const items: Item[] = [
    {
      label: "$(circle-slash) Off",
      description: current === "" ? "current" : undefined,
      value: "",
    },
    { label: "$(edit) Enter a revision...", action: "type" },
    { label: "$(file) Browse for a snapshot or schema...", action: "browse" },
    { label: "Git", kind: vscode.QuickPickItemKind.Separator },
    ...refs.map((ref) => ({
      label: `$(git-branch) ${ref}`,
      description: ref === current ? "current" : undefined,
      value: ref,
    })),
  ];
  const picked = await vscode.window.showQuickPick(items, {
    title: "sqlty: Check Breaking Changes Against",
    placeHolder: current ? `Now: ${current}` : "A baseline, as sqlty breaking --against",
  });
  let against = picked?.value;
  if (picked?.action === "type") {
    against = await vscode.window.showInputBox({
      title: "sqlty: Check Breaking Changes Against",
      prompt: "A git revision: a branch, tag, SHA or HEAD~1",
      value: current,
    });
  } else if (picked?.action === "browse") {
    const [uri] =
      (await vscode.window.showOpenDialog({
        defaultUri: folder.uri,
        canSelectFiles: true,
        canSelectFolders: true,
        openLabel: "Check against",
      })) ?? [];
    against = uri === undefined ? undefined : relative(folder.uri.fsPath, uri.fsPath) || ".";
  }
  if (against === undefined) {
    return;
  }
  await config.update("breaking.against", against, vscode.ConfigurationTarget.Workspace);
}

/** The branches and tags of the repository at `cwd`, the last committed to first. */
async function gitRefs(cwd: string): Promise<string[]> {
  const git = { command: "git", args: [] };
  const args = ["for-each-ref", "--format=%(refname)\t%(refname:short)", "--sort=-committerdate"];
  try {
    const { stdout, code } = await run(
      git,
      [...args, "refs/heads", "refs/remotes", "refs/tags"],
      cwd,
    );
    return code !== 0
      ? []
      : stdout
          .split("\n")
          .map((line) => line.split("\t"))
          .filter(([full, short]) => short && !full.endsWith("/HEAD"))
          .map(([, short]) => short)
          .slice(0, 50);
  } catch {
    return [];
  }
}
