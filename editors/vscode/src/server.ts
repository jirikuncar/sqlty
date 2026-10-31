import { execFile } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);

/** The oldest `sqlty server` with every feature the extension uses: bump it with each new one. */
export const MINIMUM_VERSION = "0.3.0";

export type Settings = {
  path: string[];
  interpreter: string[];
  breaking: { against: string; direction: "both" | "forward" | "backward" };
};

export type Server = {
  command: string;
  args: string[];
  /** Where it was found, for the output channel. */
  origin: string;
  version: string;
};

/** The server to run, or why there is none. */
export type Found = { server: Server } | { error: string };

/**
 * The first `sqlty` of at least `MINIMUM_VERSION`, in the order of ruff's extension:
 * `sqlty.path`, then the interpreter's environment, then `PATH`.
 */
export async function findServer(
  settings: Settings,
  interpreter: string[] | undefined,
): Promise<Found> {
  const candidates = settings.path.map((path) => ({
    command: expandHome(path),
    args: [] as string[],
    origin: "sqlty.path",
  }));
  if (interpreter !== undefined && interpreter.length > 0) {
    const [command, ...args] = interpreter;
    candidates.push({ command, args: [...args, "-m", "sqlty"], origin: "interpreter" });
  }
  candidates.push({ command: "sqlty", args: [], origin: "PATH" });
  const tooOld: string[] = [];
  for (const candidate of candidates) {
    const version = await versionOf(candidate.command, candidate.args);
    if (version === undefined) {
      continue;
    }
    const server = { ...candidate, args: [...candidate.args, "server"], version };
    if (compareVersions(version, MINIMUM_VERSION) >= 0) {
      return { server };
    }
    tooOld.push(`${version} (${server.command}, from ${server.origin})`);
  }
  if (tooOld.length > 0) {
    const found = tooOld.join(", ");
    return {
      error: `sqlty: the extension needs sqlty ${MINIMUM_VERSION} or newer, found ${found}. Upgrade it: \`uv add --dev 'sqlty>=${MINIMUM_VERSION}'\` or \`pip install -U sqlty\`.`,
    };
  }
  return {
    error: "sqlty: no sqlty found. Install sqlty in the selected interpreter, or set sqlty.path.",
  };
}

/** `-1`, `0` or `1` as release `a` is older than, the same as, or newer than `b`. */
export function compareVersions(a: string, b: string): number {
  const left = release(a);
  const right = release(b);
  for (let index = 0; index < Math.max(left.length, right.length); index++) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) {
      return Math.sign(difference);
    }
  }
  return 0;
}

/** `0.3.0` of `0.3.0a1` or `0.3.0.dev2`: a pre-release counts as its release. */
function release(version: string): number[] {
  return (/^\d+(\.\d+)*/.exec(version)?.[0] ?? "0").split(".").map(Number);
}

/** The version `sqlty --version` prints, or `undefined` when it is not a `sqlty`. */
async function versionOf(command: string, args: string[]): Promise<string | undefined> {
  try {
    const { stdout } = await run(command, [...args, "--version"], { timeout: 30_000 });
    return /^sqlty (\d\S*)/m.exec(stdout)?.[1];
  } catch {
    return undefined;
  }
}

function expandHome(path: string): string {
  return path === "~" || path.startsWith("~/") ? join(homedir(), path.slice(1)) : path;
}
