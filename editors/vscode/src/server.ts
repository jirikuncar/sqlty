import { execFile } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);

export type Settings = {
  path: string[];
  interpreter: string[];
  importStrategy: "fromEnvironment" | "useBundled";
  breaking: { against: string; direction: "both" | "forward" | "backward" };
};

export type Server = {
  command: string;
  args: string[];
  /** Where it was found, for the output channel. */
  origin: string;
};

/**
 * The first `sqlty` that has `sqlty server`, in the order of ruff's extension: `sqlty.path`,
 * then the interpreter's environment, then `PATH`.
 */
export async function findServer(
  settings: Settings,
  interpreter: string[] | undefined,
): Promise<Server | undefined> {
  const candidates: Server[] = settings.path.map((path) => ({
    command: expandHome(path),
    args: ["server"],
    origin: "sqlty.path",
  }));
  if (settings.importStrategy === "fromEnvironment") {
    if (interpreter !== undefined && interpreter.length > 0) {
      const [command, ...args] = interpreter;
      candidates.push({ command, args: [...args, "-m", "sqlty", "server"], origin: "interpreter" });
    }
    candidates.push({ command: "sqlty", args: ["server"], origin: "PATH" });
  }
  for (const candidate of candidates) {
    if (await serves(candidate)) {
      return candidate;
    }
  }
  return undefined;
}

/** Whether `candidate` runs and knows `sqlty server`: releases before it do not. */
async function serves(candidate: Server): Promise<boolean> {
  try {
    await run(candidate.command, [...candidate.args, "--help"], { timeout: 30_000 });
    return true;
  } catch {
    return false;
  }
}

function expandHome(path: string): string {
  return path === "~" || path.startsWith("~/") ? join(homedir(), path.slice(1)) : path;
}
