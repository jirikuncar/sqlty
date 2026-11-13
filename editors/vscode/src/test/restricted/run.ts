import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { downloadAndUnzipVSCode } from "@vscode/test-electron";

/**
 * `runTests` always passes `--disable-workspace-trust`, so this launches VS Code itself: with
 * trust enabled and no prompt, the fixture opens in Restricted Mode.
 */
async function main(): Promise<void> {
  const executable = await downloadAndUnzipVSCode();
  const profile = mkdtempSync(join(tmpdir(), "sqlty-restricted-"));
  mkdirSync(join(profile, "data", "User"), { recursive: true });
  writeFileSync(
    join(profile, "data", "User", "settings.json"),
    JSON.stringify({ "security.workspace.trust.startupPrompt": "never" }),
  );
  const { status } = spawnSync(
    executable,
    [
      resolve("fixture"),
      "--no-sandbox",
      "--disable-gpu-sandbox",
      "--disable-updates",
      "--skip-welcome",
      "--skip-release-notes",
      "--disable-extensions",
      `--user-data-dir=${join(profile, "data")}`,
      `--extensions-dir=${join(profile, "extensions")}`,
      `--extensionDevelopmentPath=${resolve(".")}`,
      `--extensionTestsPath=${resolve("out/test/restricted/index.js")}`,
    ],
    { stdio: "inherit" },
  );
  process.exit(status ?? 1);
}

void main();
