// The lines of src/ the desktop tests run (`vscode-test --coverage`), against a floor in percent.
import { readFileSync } from "node:fs";
import { relative, resolve } from "node:path";

const floor = Number(process.argv[2]);
const summary = JSON.parse(readFileSync("coverage/coverage-summary.json", "utf8"));
let total = 0;
let covered = 0;
for (const [path, { lines }] of Object.entries(summary)) {
  const file = relative(resolve("src"), path);
  if (path === "total" || file.startsWith("..") || file.startsWith("test")) {
    continue;
  }
  total += lines.total;
  covered += lines.covered;
  console.log(`${file.padEnd(16)} ${lines.pct.toFixed(2).padStart(6)}%`);
}
const percent = (100 * covered) / total;
console.log(`${"total".padEnd(16)} ${percent.toFixed(2).padStart(6)}%`);
if (percent < floor) {
  console.error(`Line coverage ${percent.toFixed(2)}% is below ${floor}%`);
  process.exit(1);
}
