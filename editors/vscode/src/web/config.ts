import * as vscode from "vscode";

/** The engine of `sqlty_wasm.wasm`: a JSON request in, a JSON answer out. */
export type Engine = (request: Record<string, unknown>) => any;

/** The sqlty table of a file, as the engine's `config` op answers it. */
export type Table = Record<string, unknown>;

/**
 * What a file is checked with: the merged table, the text of its schema files, and the
 * directory of the file holding the table, against which the engine routes each file to
 * its database and overrides.
 */
export type Project = { settings: Table; schema: string; base?: string } | { error: string };

/**
 * The project of the file in `directory`, as `sqlty check` finds it: the engine's `project`
 * op asks for each file it reads, applies `extend`, and gives `schema` relative to the
 * directory of the file that holds the table.
 */
export async function project(engine: Engine, directory: vscode.Uri): Promise<Project> {
  const files: Record<string, string | null> = {};
  for (;;) {
    const answer = engine({ op: "project", path: directory.path, files });
    if (answer === null || "warning" in answer) {
      return { settings: {}, schema: "" };
    }
    if ("error" in answer) {
      return { error: answer.error };
    }
    if ("need" in answer) {
      files[answer.need] = await readText(directory.with({ path: answer.need }));
      continue;
    }
    const table = answer.config as Table;
    const base = parent(directory.with({ path: answer.file }));
    return { settings: table, schema: await readSchemas(schemasOf(table, base)), base: base.path };
  }
}

type Schemas = { base: vscode.Uri; entries: string[] };

async function readText(uri: vscode.Uri): Promise<string | null> {
  try {
    return new TextDecoder().decode(await vscode.workspace.fs.readFile(uri));
  } catch {
    return null;
  }
}

function schemasOf(table: Table, base: vscode.Uri): Schemas | undefined {
  const schema = table.schema;
  if (typeof schema === "string") {
    return { base, entries: [schema] };
  }
  return Array.isArray(schema) ? { base, entries: schema as string[] } : undefined;
}

/**
 * The schema files concatenated: each entry is a file, a directory (its `*.sql` files,
 * recursively) or a glob, in the order `sqlty check` reads them.
 */
async function readSchemas(schemas: Schemas | undefined): Promise<string> {
  if (schemas === undefined) {
    return "";
  }
  const files = new Map<string, vscode.Uri>();
  for (const entry of schemas.entries) {
    for (const uri of await expand(schemas.base, entry)) {
      files.set(uri.toString(), uri);
    }
  }
  const texts = await Promise.all([...files.values()].map(readText));
  return texts.filter((text) => text !== null).join("\n");
}

async function expand(base: vscode.Uri, entry: string): Promise<vscode.Uri[]> {
  if (/[*?[]/.test(entry)) {
    const found = await vscode.workspace.findFiles(new vscode.RelativePattern(base, entry));
    return found.sort(byPath);
  }
  const uri = vscode.Uri.joinPath(base, entry);
  try {
    if ((await vscode.workspace.fs.stat(uri)).type & vscode.FileType.Directory) {
      return (await sqlFiles(uri)).sort(byPath);
    }
  } catch {
    return [];
  }
  return [uri];
}

async function sqlFiles(directory: vscode.Uri): Promise<vscode.Uri[]> {
  const found: vscode.Uri[] = [];
  for (const [name, type] of await vscode.workspace.fs.readDirectory(directory)) {
    const uri = vscode.Uri.joinPath(directory, name);
    if (type & vscode.FileType.Directory) {
      found.push(...(await sqlFiles(uri)));
    } else if (name.endsWith(".sql")) {
      found.push(uri);
    }
  }
  return found;
}

function byPath(a: vscode.Uri, b: vscode.Uri): number {
  return a.path < b.path ? -1 : a.path > b.path ? 1 : 0;
}

function parent(uri: vscode.Uri): vscode.Uri {
  return vscode.Uri.joinPath(uri, "..");
}
