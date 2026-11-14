// The lines of src/ that the desktop and Restricted Mode runs execute (their NODE_V8_COVERAGE
// directory), against a floor in percent.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { Report } from "c8";

const [directory, floor] = process.argv.slice(2);
await new Report({
  tempDirectory: directory,
  reporter: ["json-summary", "text"],
  reporterOptions: { text: { skipFull: true } },
  reportsDirectory: "coverage",
  src: [resolve("src")],
  exclude: ["src/test/**"],
  excludeNodeModules: true,
  mergeAsync: true,
}).run();

const summary = JSON.parse(readFileSync("coverage/coverage-summary.json", "utf8"));
const { lines } = summary.total;
if (lines.pct < Number(floor)) {
  console.error(`Line coverage ${lines.pct}% is below ${floor}%`);
  process.exit(1);
}
