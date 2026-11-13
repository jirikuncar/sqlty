import * as vscode from "vscode";

/** The engine of `sqlty_wasm.wasm`: a JSON request in, a JSON answer out. */
export type Engine = (request: Record<string, unknown>) => any;

/** The sqlty table of a file, as the engine's `config` op answers it. */
export type Table = Record<string, unknown>;

/** What a file is checked with: the merged table, and the text of its schema files. */
export type Project = { settings: Table; schema: string } | { error: string };

/** The files a directory may hold a sqlty table in, tried in this order, as `sqlty check` does. */
const CONFIG_FILES = ["sqlty.toml", "pyproject.toml", "Cargo.toml"];

/**
 * The project of the file in `directory`, as `sqlty check` finds it: the closest directory
 * with a sqlty table wins, `extend` is applied on top of the file it names, and `schema` is
 * read relative to the file that set it.
 */
export async function project(engine: Engine, directory: vscode.Uri): Promise<Project> {
  try {
    for (let current = directory; ; current = parent(current)) {
      for (const name of CONFIG_FILES) {
        const found = await load(engine, vscode.Uri.joinPath(current, name), false);
        if (found !== undefined) {
          const { table, schemas } = await extended(engine, found, current, []);
          return { settings: table, schema: await readSchemas(schemas) };
        }
      }
      if (parent(current).path === current.path) {
        return { settings: {}, schema: "" };
      }
    }
  } catch (error) {
    return { error: error instanceof Error ? error.message : String(error) };
  }
}

type Schemas = { base: vscode.Uri; entries: string[] };

/** `table` with the files it extends applied under it; `schema` keeps the directory it is relative to. */
async function extended(
  engine: Engine,
  table: Table,
  directory: vscode.Uri,
  chain: string[],
): Promise<{ table: Table; schemas: Schemas | undefined }> {
  const own = schemasOf(table, directory);
  const extend = table.extend;
  if (typeof extend !== "string") {
    return { table, schemas: own };
  }
  const target = vscode.Uri.joinPath(directory, extend);
  if (chain.includes(target.toString())) {
    throw new Error(`${target.path}: extend = '${extend}' extends itself in a cycle`);
  }
  const found = await load(engine, target, true);
  if (found === undefined) {
    throw new Error(`extend = '${extend}': config file does not exist: ${target.path}`);
  }
  const base = await extended(engine, found, parent(target), [...chain, target.toString()]);
  const merged: Table = { ...base.table, ...table };
  delete merged.extend;
  for (const key of ["rules", "types"]) {
    if (isTable(base.table[key]) && isTable(table[key])) {
      merged[key] = { ...base.table[key], ...table[key] };
    }
  }
  return { table: merged, schemas: own ?? base.schemas };
}

/**
 * The sqlty table of `uri`, or `undefined` when there is no such file, or a `pyproject.toml`
 * or `Cargo.toml` has none. A file `extend` names (`required`) must have one.
 */
async function load(
  engine: Engine,
  uri: vscode.Uri,
  required: boolean,
): Promise<Table | undefined> {
  let bytes: Uint8Array;
  try {
    bytes = await vscode.workspace.fs.readFile(uri);
  } catch {
    return undefined;
  }
  const answer = engine({
    op: "config",
    source: new TextDecoder().decode(bytes),
    path: uri.path,
    required,
    dialect: "",
  });
  if (answer !== null && "error" in answer) {
    throw new Error(answer.error);
  }
  return answer === null ? undefined : (answer.config as Table);
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
  const texts = await Promise.all(
    [...files.values()].map(async (uri) => {
      try {
        return new TextDecoder().decode(await vscode.workspace.fs.readFile(uri));
      } catch {
        return undefined;
      }
    }),
  );
  return texts.filter((text) => text !== undefined).join("\n");
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

function isTable(value: unknown): value is Table {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
