// Records docs/assets/vscode-demo.gif in openvscode-server with this extension.
//
//   npm run package
//   OPENVSCODE_SERVER=~/openvscode-server-v1.105.1-linux-x64 npm run demo
//
// Needs `sqlty` on PATH, Playwright with Chromium, ffmpeg and gifsicle.
// Writes vscode-demo.mp4 too when DEMO_MP4 is set.

const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { chromium } = require("playwright");

const W = 1280;
const H = 420;
const SCALE = 2;
const TITLE_BAR = 34;
const PORT = 3999;

const root = path.resolve(__dirname, "..");
const gif = path.resolve(root, "../../docs/assets/vscode-demo.gif");
const vsix = fs.readdirSync(root).find((file) => file.endsWith(".vsix"));
const server = path.join(process.env.OPENVSCODE_SERVER ?? "", "bin/openvscode-server");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "sqlty-demo-"));
const project = path.join(tmp, "project");
const frames = path.join(tmp, "frames");

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const write = (file, text) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
};

write(path.join(project, "pyproject.toml"), '[tool.sqlty]\nschema = "schema.sql"\n');
write(
  path.join(project, "schema.sql"),
  "CREATE TABLE users (\n    id INTEGER PRIMARY KEY,\n    name TEXT NOT NULL,\n    email TEXT NOT NULL\n);\n",
);
write(
  path.join(project, "users.py"),
  'from sqlty import SQL\n\nall_users = SQL[tuple[int, str]]("""--sql\nSELECT id, name FROM users ORDER BY name\n""")\n',
);
const settings = JSON.stringify(
  {
    "workbench.colorTheme": "Default Dark Modern",
    "workbench.startupEditor": "none",
    "workbench.activityBar.location": "hidden",
    "workbench.tips.enabled": false,
    "workbench.editor.enablePreview": false,
    "window.commandCenter": false,
    "editor.fontSize": 15,
    "editor.lineHeight": 24,
    "editor.minimap.enabled": false,
    "editor.suggest.preview": false,
    "editor.quickSuggestionsDelay": 10,
    "editor.hover.delay": 300,
    "editor.stickyScroll.enabled": false,
    "editor.wordBasedSuggestions": "off",
    "editor.occurrencesHighlight": "off",
    "editor.selectionHighlight": false,
    "editor.overviewRulerBorder": false,
    "editor.hideCursorInOverviewRuler": true,
    "breadcrumbs.enabled": false,
    "files.autoSave": "off",
    "files.hotExit": "off",
    "security.workspace.trust.enabled": false,
    "telemetry.telemetryLevel": "off",
    "chat.disableAIFeatures": true,
    "[python]": {
      "editor.codeActionsOnSave": {
        "source.format.sqlty": "explicit",
        "source.fixAll.sqlty": "explicit",
      },
    },
  },
  null,
  2,
);
write(path.join(project, ".vscode/settings.json"), settings);
write(path.join(tmp, "data/data/Machine/settings.json"), settings);

const dirs = [
  `--extensions-dir=${path.join(tmp, "extensions")}`,
  `--server-data-dir=${path.join(tmp, "data")}`,
  `--user-data-dir=${path.join(tmp, "data")}`,
];
execFileSync(server, [...dirs, "--install-extension", path.join(root, vsix)], {
  stdio: "inherit",
});
const vscode = spawn(
  server,
  [...dirs, "--host=127.0.0.1", `--port=${PORT}`, "--without-connection-token"],
  { detached: true, stdio: "ignore" },
);

async function record(page) {
  const key = (name) => page.keyboard.press(name);
  const type = async (text, delay = 55) => {
    for (const ch of text) {
      await page.keyboard.type(ch);
      await sleep(delay + Math.random() * 40);
    }
  };
  const command = async (name) => {
    await key("F1");
    await sleep(400);
    await page.keyboard.type(name);
    await sleep(600);
    await key("Enter");
    await sleep(600);
  };
  const open = async (file, side = false) => {
    await key("Control+P");
    await sleep(500);
    await page.keyboard.type(file);
    await sleep(800);
    await key(side ? "Control+Enter" : "Enter");
    await sleep(1500);
  };
  const goTo = async (position) => {
    await key("Control+G");
    await sleep(200);
    await page.keyboard.type(position);
    await key("Enter");
  };
  const park = () => page.mouse.move(W - 5, H - 5);
  const hoverAndFix = async () => {
    const squiggle = page.locator(".view-overlays .squiggly-error").last();
    await squiggle.waitFor({ timeout: 15_000 });
    await sleep(800);
    const box = await squiggle.boundingBox();
    const [x, y] = [box.x + box.width / 2, box.y + box.height / 2];
    await page.mouse.move(x, y, { steps: 12 });
    await page.waitForSelector(".monaco-hover:not(.hidden)", { timeout: 10_000 });
    await sleep(2400);
    await page.mouse.click(x, y);
    await park();
    await key("Control+.");
    await sleep(1600);
    await key("Enter");
    await sleep(2000);
    await key("Control+S");
    await sleep(1500);
  };

  await page.goto(`http://127.0.0.1:${PORT}/?folder=${project}`);
  await page.waitForSelector(".monaco-workbench", { timeout: 60_000 });
  await sleep(4000);
  await command("View: Close All Editors");
  await command("View: Close Primary Side Bar");
  await command("View: Close Secondary Side Bar");
  await open("users.py");
  await command("View: Close Other Editors in Group");
  await open("schema.sql", true);
  await key("Control+1");
  await sleep(4000);
  await goTo("4:16");
  await park();
  await sleep(1500);

  const shots = [];
  let recording = true;
  const capture = (async () => {
    while (recording) {
      const t = Date.now() / 1000;
      const file = path.join(frames, `${String(shots.length).padStart(5, "0")}.png`);
      fs.writeFileSync(file, await page.screenshot());
      shots.push({ file, t });
    }
  })();
  await sleep(1500);

  await type(", em", 90);
  await page.waitForSelector(".suggest-widget.visible", { timeout: 10_000 });
  await sleep(1000);
  await key("Enter");
  await page.locator(".view-overlays .squiggly-error").first().waitFor({ timeout: 15_000 });
  await sleep(3000);
  await key("Control+S");
  await sleep(3500);

  await key("Control+2");
  await goTo("4");
  await key("End");
  await sleep(800);
  await key("Control+Shift+ArrowLeft");
  await sleep(250);
  await key("Control+Shift+ArrowLeft");
  await sleep(600);
  await key("Backspace");
  await key("Backspace");
  await sleep(800);
  await key("Control+S");
  await hoverAndFix();

  await key("Control+2");
  await key("Control+W");
  await sleep(500);
  await key("Control+End");
  await sleep(1000);
  await key("Enter");
  await type('by_email = SQL[tuple[str]]("--sql\\nSELECT id FROM users u WHERE u.');
  await page.waitForSelector(".suggest-widget.visible", { timeout: 10_000 });
  await sleep(1400);
  await key("Enter");
  await sleep(300);
  await type(' = %(email)s")');
  await sleep(2500);
  await hoverAndFix();

  recording = false;
  await capture;
  return shots;
}

function encode(shots) {
  const list = path.join(frames, "list.txt");
  const entries = shots.map((shot, i) => {
    const next = shots[i + 1]?.t ?? shot.t + 1.5;
    return `file '${shot.file}'\nduration ${Math.max(next - shot.t, 0.02).toFixed(3)}`;
  });
  fs.writeFileSync(list, `${entries.join("\n")}\nfile '${shots.at(-1).file}'\n`);
  const crop = `crop=${W * SCALE}:${(H - TITLE_BAR) * SCALE}:0:${TITLE_BAR * SCALE}`;
  const input = ["-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", list];
  const palette =
    "split[a][b];[a]palettegen=stats_mode=diff:max_colors=128[p];[b][p]paletteuse=dither=none:diff_mode=rectangle";
  const raw = path.join(tmp, "raw.gif");
  execFileSync("ffmpeg", [...input, "-vf", `${crop},fps=12,${palette}`, raw]);
  execFileSync("gifsicle", ["-O3", raw, "-o", gif]);
  console.log(`wrote ${gif}`);
  if (process.env.DEMO_MP4) {
    const mp4 = gif.replace(/\.gif$/, ".mp4");
    execFileSync("ffmpeg", [
      ...input,
      "-vf",
      `${crop},fps=30`,
      "-c:v",
      "libx264",
      "-preset",
      "slow",
      "-crf",
      "18",
      "-pix_fmt",
      "yuv420p",
      "-movflags",
      "+faststart",
      mp4,
    ]);
    console.log(`wrote ${mp4}`);
  }
}

(async () => {
  fs.mkdirSync(frames);
  await sleep(3000);
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({
      viewport: { width: W, height: H },
      deviceScaleFactor: SCALE,
    });
    encode(await record(page));
  } finally {
    await browser.close();
    process.kill(-vscode.pid);
    fs.rmSync(tmp, { recursive: true, force: true });
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
