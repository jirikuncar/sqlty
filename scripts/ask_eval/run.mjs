// Ask each question of questions.json to a model in headless Chromium, as the playground does,
// and write the attempts to a JSON file for score.py.
//
//   node scripts/ask_eval/run.mjs --model qwen2.5-coder-0.5b --models DIR --wllama DIR [--isolated] [--out FILE]
//
// --models holds the GGUF files of ask.js MODELS; --wllama is @wllama/wllama/esm. --isolated
// sends COOP/COEP, which wllama needs for threads (GitHub Pages cannot send them).
import { createReadStream, mkdtempSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";
import { parseArgs } from "node:util";

const { values: args } = parseArgs({
  options: {
    model: { type: "string" },
    models: { type: "string" },
    wllama: { type: "string" },
    isolated: { type: "boolean", default: false },
    threads: { type: "string" },
    repairs: { type: "string", default: "2" },
    out: { type: "string" },
    executable: { type: "string", default: "/opt/pw-browsers/chromium" },
  },
});
const ROOT = resolve(import.meta.dirname, "../..");
const MOUNTS = {
  "/playground/": join(ROOT, "docs/assets/playground"),
  "/eval/": import.meta.dirname,
  "/wllama/": resolve(args.wllama),
  "/models/": resolve(args.models),
};
const TYPES = { ".js": "text/javascript", ".mjs": "text/javascript", ".json": "application/json", ".wasm": "application/wasm", ".html": "text/html" };

const server = createServer((request, response) => {
  const path = decodeURIComponent(new URL(request.url, "http://x").pathname);
  const mount = Object.keys(MOUNTS).find((prefix) => path.startsWith(prefix));
  const file = mount && join(MOUNTS[mount], path.slice(mount.length));
  let size;
  try {
    size = statSync(file).size;
  } catch {
    response.writeHead(404).end();
    return;
  }
  const headers = { "content-type": TYPES[extname(file)] ?? "application/octet-stream", "content-length": size };
  if (args.isolated) {
    headers["cross-origin-opener-policy"] = "same-origin";
    headers["cross-origin-embedder-policy"] = "require-corp";
  }
  response.writeHead(200, headers);
  createReadStream(file).pipe(response);
}).listen(0);
const base = `http://localhost:${server.address().port}`;

const { chromium } = createRequire(import.meta.url)("playwright");
// A persistent profile, as a person's browser has: a fresh context's storage quota is too
// small to cache the 1 GB+ models.
const profile = mkdtempSync(join(tmpdir(), "ask-eval-"));
const browser = await chromium.launchPersistentContext(profile, { executablePath: args.executable });
const page = await browser.newPage();
page.on("console", (message) => message.type() === "error" && console.error(message.text()));
await page.goto(`${base}/eval/page.html`);
const results = await page.evaluate(
  async ({ model, repairs, threads }) => window.evaluate({ model, repairs, threads }),
  { model: args.model, repairs: Number(args.repairs), threads: args.threads && Number(args.threads) },
);
await browser.close();
rmSync(profile, { recursive: true });
server.close();
const out = args.out ?? `ask-eval-${args.model}${args.isolated ? "" : "-1thread"}.json`;
writeFileSync(out, JSON.stringify({ isolated: args.isolated, ...results }, null, 2));
console.log(`${out}: load ${results.loadSeconds.toFixed(1)}s, ${results.answers.length} answers`);
