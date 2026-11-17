#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");
const { constants } = require("node:os");

const target = `sqlty-${process.platform}-${process.arch}`;
const executable = process.platform === "win32" ? "sqlty.exe" : "sqlty";

let binary;
try {
  binary = require.resolve(`${target}/bin/${executable}`);
} catch {
  console.error(
    `sqlty: no prebuilt binary for ${process.platform}-${process.arch} (package ${target}).\n` +
      "Install the Python package instead: uv tool install sqlty",
  );
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  throw result.error;
}
process.exitCode = result.signal ? 128 + (constants.signals[result.signal] ?? 0) : result.status;
