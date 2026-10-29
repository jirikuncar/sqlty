import { defineConfig } from "@vscode/test-cli";

export default defineConfig({
  files: "out/test/**/*.test.js",
  workspaceFolder: "fixture",
  launchArgs: ["--disable-extensions"],
  mocha: { timeout: 60_000 },
});
