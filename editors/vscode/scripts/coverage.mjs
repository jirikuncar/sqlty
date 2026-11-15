// The lines of src/ that the test runs execute, against a floor in percent:
// `node scripts/coverage.mjs FLOOR DIR...`, a NODE_V8_COVERAGE directory per run. Each run
// is mapped to src/ on its own, then the runs are merged line by line.
import { readFileSync } from "node:fs";
import { basename, resolve } from "node:path";

import { Report } from "c8";
import libCoverage from "istanbul-lib-coverage";
import libReport from "istanbul-lib-report";
import reports from "istanbul-reports";

const [floor, ...directories] = process.argv.slice(2);
const map = libCoverage.createCoverageMap({});
for (const directory of directories) {
  const output = `coverage/${basename(directory)}`;
  await new Report({
    tempDirectory: directory,
    reporter: ["json"],
    reportsDirectory: output,
    src: [resolve("src")],
    include: ["src/**"],
    exclude: ["src/test/**"],
    excludeNodeModules: true,
    mergeAsync: true,
  }).run();
  map.merge(JSON.parse(readFileSync(`${output}/coverage-final.json`, "utf8")));
}
const context = libReport.createContext({ dir: "coverage", coverageMap: map });
reports.create("text", { skipFull: true }).execute(context);
const { pct } = map.getCoverageSummary().lines;
if (pct < Number(floor)) {
  console.error(`Line coverage ${pct}% is below ${floor}%`);
  process.exit(1);
}
