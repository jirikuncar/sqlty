import { load } from "./engine.js";
import { decode, encode, MAX_LINK } from "./share.js";
import { fold, unfold } from "./steps.js";

const MONACO = "https://cdn.jsdelivr.net/npm/monaco-editor@0.57.0/min/vs";
const LEGEND = ["keyword", "string", "number", "comment", "operator", "parameter"];

const SCHEMA = `CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id),
    total NUMERIC(10, 2) NOT NULL,
    placed_at TIMESTAMP NOT NULL
);
`;

const SOURCE = `from sqlty import SQL

# sqlty rewrite wraps a query in SQL[T], its row type; sqlty check keeps T up to date.
users = SQL[tuple[int, str, str | None]]("--sql\\nSELECT id, name, email FROM users WHERE name = %s")

# A string that starts with --sql is a query. Parameters are typed by what they are
# compared with: "Types" shows them.
totals = """--sql
SELECT u.name, SUM(o.total) AS spent, MAX(o.placed_at) AS last_order
FROM users AS u
LEFT JOIN orders AS o ON o.user_id = u.id
WHERE o.placed_at > %(since)s
GROUP BY u.name"""

# A typo in a column name.
typo = "--sql\\nSELECT nme FROM users"

# A row type that does not match the query.
stale = SQL[tuple[int, str]]("--sql\\nSELECT id, email FROM users")

# In a query, Ctrl+Space completes tables and columns, and hovering a name shows its type.
joined = "--sql\\nSELECT o.id, u.name FROM orders AS o JOIN users AS u ON u.id = o.user_id"
`;

const CONFIG = `# The [tool.sqlty] keys that need no files: dialect, import-style and rules.
[rules]
stale-rewrite = "error"
`;

const root = document.getElementById("sqlty-playground");
const [monaco, ask, shared, examples, linkedDeck] = await Promise.all([
  loadMonaco(),
  engine(),
  decode(location.hash.slice(1)),
  fetch(new URL("examples.json", import.meta.url)).then((response) => response.json()),
  fetchDeck(new URLSearchParams(location.search).get("steps")).catch((error) => ({ error })),
]);
const SIZES = [11, 12, 13, 14, 16, 18, 20, 22, 24, 28, 32];
const saved = remembered();
let size = SIZES.includes(saved.size) ? saved.size : 13;
let steps = [];
let step = 0;
let current = { diagnostics: [], queries: [], highlights: [], rewritten: "", stripped: "", fixed: "" };
const refreshed = new monaco.Emitter();

for (const [name, base] of [["sqlty-light", "vs"], ["sqlty-dark", "vs-dark"]]) {
  monaco.editor.defineTheme(name, {
    base,
    inherit: true,
    rules: [
      { token: "keyword", foreground: base === "vs" ? "0000ff" : "569cd6" },
      { token: "number", foreground: base === "vs" ? "098658" : "b5cea8" },
      { token: "comment", foreground: base === "vs" ? "008000" : "6a9955" },
      { token: "operator", foreground: base === "vs" ? "000000" : "d4d4d4" },
      { token: "parameter", foreground: base === "vs" ? "001080" : "9cdcfe" },
      { token: "string", foreground: base === "vs" ? "a31515" : "ce9178" },
    ],
    colors: {},
  });
}
const theme = () => (document.body.dataset.mdColorScheme === "slate" ? "sqlty-dark" : "sqlty-light");
new MutationObserver(() => monaco.editor.setTheme(theme())).observe(document.body, {
  attributes: true,
  attributeFilter: ["data-md-color-scheme"],
});

const options = {
  theme: theme(),
  automaticLayout: true,
  minimap: { enabled: false },
  fontSize: size,
  scrollBeyondLastLine: false,
  wordBasedSuggestions: "off",
  "semanticHighlighting.enabled": true,
};
const models = {
  python: monaco.editor.createModel(SOURCE, "python"),
  schema: monaco.editor.createModel(SCHEMA, "sql"),
  config: monaco.editor.createModel(CONFIG, "ini"),
};
const editor = monaco.editor.create(root.querySelector("[data-source]"), { ...options, model: models.python });
const views = new Map();
const files = root.querySelectorAll("[data-file]");
const tabs = root.querySelectorAll("[data-tab]");
const rewritten = monaco.editor.create(root.querySelector("[data-rewritten]"), {
  ...options,
  language: "python",
  readOnly: true,
});
const output = monaco.editor.create(root.querySelector("[data-json]"), {
  ...options,
  language: "json",
  readOnly: true,
});
const dialect = root.querySelector("[data-dialect]");
const linked = root.querySelector("[data-link-status]");
const example = root.querySelector("[data-example]");
const stepper = root.querySelector("[data-steps]");
const picker = root.querySelector("[data-step-select]");
const notes = root.querySelector("[data-notes]");
const present = root.querySelector("[data-present]");
examples.unshift({ name: "Tour", source: SOURCE, schema: SCHEMA, dialect: "postgres", config: CONFIG });
examples.push({ name: "Tour in steps", deck: new URL("steps.json", import.meta.url) }, { name: "Open a file...", file: true });
const option = (value, textContent) => Object.assign(document.createElement("option"), { value, textContent });
const custom = Object.assign(option(-1, "Steps"), { hidden: true });
example.replaceChildren(custom, ...examples.map(({ name }, index) => option(index, name)));
example.value = 0;
const chosen = () => (steps.length ? -1 : 0);
example.addEventListener("change", async () => {
  const picked = examples[example.value];
  try {
    if (picked.file) await openFile();
    else if (picked.deck) begin(await fetchDeck(picked.deck));
    else {
      begin(null);
      restore(picked);
    }
  } catch (error) {
    linked.textContent = String(error.message ?? error);
  }
});

function request(op, offset = 0) {
  return {
    op,
    offset,
    source: models.python.getValue(),
    schema: models.schema.getValue(),
    dialect: dialect.value,
    config: models.config.getValue(),
  };
}

// Show a file of `models` in the editor, keeping the cursor and scroll of the one it hides.
function open(name) {
  views.set(editor.getModel(), editor.saveViewState());
  editor.setModel(models[name]);
  editor.restoreViewState(views.get(models[name]) ?? null);
  for (const tab of files) tab.setAttribute("aria-selected", String(tab.dataset.file === name));
}

function show(name) {
  for (const tab of tabs) tab.setAttribute("aria-selected", String(tab.dataset.tab === name));
  for (const panel of root.querySelectorAll("[data-panel]")) panel.hidden = panel.dataset.panel !== name;
}

// Walk through `deck`, a list of steps (steps.js), from its step `at`; null leaves the steps.
function begin(deck, at = 0) {
  steps = deck ? unfold(deck) : [];
  custom.textContent = deck?.title || "Steps";
  custom.hidden = !deck;
  stepper.hidden = !steps.length;
  picker.replaceChildren(...steps.map(({ title }, index) => option(index, `${index + 1}/${steps.length} ${title}`)));
  if (steps.length) go(at);
  else {
    root.querySelector('[data-tab="notes"]').hidden = true;
    if (root.querySelector('[data-tab="notes"][aria-selected="true"]')) show("problems");
  }
}

function go(at) {
  step = Math.max(0, Math.min(steps.length - 1, at));
  const here = steps[step];
  picker.value = step;
  root.querySelector('[data-step="-1"]').disabled = step === 0;
  root.querySelector('[data-step="1"]').disabled = step === steps.length - 1;
  notes.replaceChildren(
    ...here.notes.split(/\n\s*\n/).filter(Boolean).map((text) => Object.assign(document.createElement("p"), { textContent: text })),
  );
  root.querySelector('[data-tab="notes"]').hidden = !here.notes;
  restore(here);
  open(here.file ?? "python");
  show(here.panel ?? (here.notes ? "notes" : "problems"));
}

// Replace a model's text as one edit, so Ctrl+Z brings the old text back.
function replace(model, text) {
  if (model.getValue() === text) return;
  model.pushStackElement();
  model.pushEditOperations([], [{ range: model.getFullModelRange(), text }], () => null);
  model.pushStackElement();
}

function zoom(by) {
  size = SIZES[Math.max(0, Math.min(SIZES.length - 1, SIZES.indexOf(size) + by))];
  for (const each of monaco.editor.getEditors()) each.updateOptions({ fontSize: size });
  root.style.setProperty("--sqlty-font", `${size}px`);
  remember({ size });
}

async function presenting(on) {
  root.classList.toggle("sqlty-present", on);
  present.setAttribute("aria-pressed", String(on));
  present.textContent = on ? "Exit" : "Present";
  try {
    if (on && !document.fullscreenElement) await root.requestFullscreen?.();
    if (!on && document.fullscreenElement) await document.exitFullscreen();
  } catch {
    // Without the Fullscreen API (iPhone), the playground still covers the page.
  }
}

async function openFile() {
  const input = Object.assign(document.createElement("input"), { type: "file", accept: ".json,application/json" });
  const file = await new Promise((resolve) => {
    input.addEventListener("change", () => resolve(input.files[0]));
    input.click();
  });
  if (file) take(JSON.parse(await file.text()));
  example.value = chosen();
}

// A JSON file: a deck with `steps`, or one session.
function take(found) {
  if (Array.isArray(found?.steps)) begin(found);
  else {
    begin(null);
    restore(found);
  }
}

monaco.languages.registerDocumentSemanticTokensProvider("python", {
  onDidChange: refreshed.event,
  getLegend: () => ({ tokenTypes: LEGEND, tokenModifiers: [] }),
  provideDocumentSemanticTokens(model) {
    if (model !== models.python) return null;
    const data = [];
    let line = 0;
    let column = 0;
    for (const [start, end, kind] of current.highlights) {
      let from = start;
      for (const piece of model.getValue().slice(start, end).split("\n")) {
        const at = model.getPositionAt(from);
        if (piece.length) {
          const deltaLine = at.lineNumber - 1 - line;
          data.push(deltaLine, deltaLine ? at.column - 1 : at.column - 1 - column);
          data.push(piece.length, LEGEND.indexOf(kind), 0);
          line = at.lineNumber - 1;
          column = at.column - 1;
        }
        from += piece.length + 1;
      }
    }
    return { data: new Uint32Array(data) };
  },
  releaseDocumentSemanticTokens() {},
});

monaco.languages.registerCompletionItemProvider("python", {
  triggerCharacters: [" ", ".", ","],
  provideCompletionItems(model, position) {
    if (model !== models.python) return { suggestions: [] };
    const found = ask(request("complete", model.getOffsetAt(position)));
    if (!found?.items) return { suggestions: [] };
    const range = monaco.Range.fromPositions(
      model.getPositionAt(found.start),
      model.getPositionAt(found.end),
    );
    const kinds = monaco.languages.CompletionItemKind;
    return {
      suggestions: found.items.map((item, rank) => ({
        label: item.label,
        kind: item.kind === "column" ? kinds.Field : item.kind === "cte" ? kinds.Module : kinds.Struct,
        detail: [item.kind, item.sql_type, item.detail].filter(Boolean).join(" "),
        insertText: item.label,
        sortText: String(rank).padStart(4, "0"),
        range,
      })),
    };
  },
});

monaco.languages.registerCodeActionProvider("python", {
  provideCodeActions(model, range, context) {
    if (model !== models.python) return { actions: [], dispose() {} };
    const actions = context.markers.flatMap((marker) => {
      const found = current.diagnostics.find(
        (diagnostic) =>
          diagnostic.rule === marker.code &&
          diagnostic.fix.length &&
          model.getPositionAt(diagnostic.start).lineNumber === marker.startLineNumber &&
          model.getPositionAt(diagnostic.start).column === marker.startColumn,
      );
      if (!found) return [];
      return [
        {
          title: `Fix ${found.rule}`,
          kind: "quickfix",
          diagnostics: [marker],
          isPreferred: true,
          edit: {
            edits: found.fix.map((edit) => ({
              resource: model.uri,
              versionId: model.getVersionId(),
              textEdit: {
                range: monaco.Range.fromPositions(
                  model.getPositionAt(edit.start),
                  model.getPositionAt(edit.end),
                ),
                text: edit.replacement,
              },
            })),
          },
        },
      ];
    });
    return { actions, dispose() {} };
  },
});

monaco.languages.registerHoverProvider("python", {
  provideHover(model, position) {
    if (model !== models.python) return null;
    const found = ask(request("hover", model.getOffsetAt(position)));
    if (!found?.contents) return null;
    return {
      range: monaco.Range.fromPositions(
        model.getPositionAt(found.start),
        model.getPositionAt(found.end),
      ),
      contents: [{ value: found.contents }],
    };
  },
});

function refresh() {
  const answer = ask(request("analyze"));
  const problems = root.querySelector("[data-problems]");
  if (answer.error) {
    problems.textContent = answer.error;
    return;
  }
  current = answer;
  const model = models.python;
  monaco.editor.setModelMarkers(
    model,
    "sqlty",
    answer.diagnostics.map((found) => {
      const start = model.getPositionAt(found.start);
      const end = model.getPositionAt(found.end);
      return {
        startLineNumber: start.lineNumber,
        startColumn: start.column,
        endLineNumber: end.lineNumber,
        endColumn: end.column,
        message: found.message,
        code: found.rule,
        source: "sqlty",
        severity: found.severity === "error" ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning,
      };
    }),
  );
  problems.replaceChildren(
    ...answer.diagnostics.map((found) => {
      const item = document.createElement("li");
      item.dataset.severity = found.severity;
      const at = model.getPositionAt(found.start);
      const rule = Object.assign(document.createElement("a"), {
        href: new URL(`../reference/rules/#${found.rule}`, location.href).href,
        textContent: found.rule,
        title: `The ${found.rule} rule`,
      });
      const button = Object.assign(document.createElement("button"), {
        type: "button",
        textContent: `${at.lineNumber}:${at.column} ${found.message}`,
      });
      button.addEventListener("click", () => {
        open("python");
        editor.setPosition(at);
        editor.revealPositionInCenter(at);
        editor.focus();
      });
      item.append(button, rule);
      return item;
    }),
  );
  if (!answer.diagnostics.length) problems.replaceChildren(Object.assign(document.createElement("li"), { textContent: "No problems." }));
  root.querySelector('[data-tab="problems"]').textContent = `Problems (${answer.diagnostics.length})`;
  root.querySelector("[data-types]").replaceChildren(...answer.queries.map(describe));
  rewritten.setValue(answer.rewritten);
  output.setValue(JSON.stringify(answer.diagnostics.map((found) => checked(found, model)), null, 2));
  refreshed.fire();
}

// A diagnostic as `sqlty check --output-format json` prints it.
function checked(found, model) {
  const at = (offset) => {
    const { lineNumber, column } = model.getPositionAt(offset);
    return { line: lineNumber, column };
  };
  return {
    file: "queries.py",
    rule: found.rule,
    message: found.message,
    severity: found.severity,
    line: found.line,
    start: at(found.start),
    end: at(found.end),
    fix: found.fix.map((edit) => ({ start: at(edit.start), end: at(edit.end), replacement: edit.replacement })),
  };
}

function session() {
  const edited = {
    source: models.python.getValue(),
    schema: models.schema.getValue(),
    dialect: dialect.value,
    config: models.config.getValue(),
  };
  return steps.length ? { ...edited, steps: fold(steps).steps, step } : edited;
}

function restore(saved) {
  for (const [model, value] of [[models.python, saved.source], [models.schema, saved.schema], [models.config, saved.config]]) {
    if (typeof value === "string") replace(model, value);
  }
  if ([...dialect.options].some((option) => option.value === saved.dialect)) dialect.value = saved.dialect;
}

// A deck, or null, from a URL that `?steps=` names, relative to the page.
async function fetchDeck(url) {
  if (!url) return null;
  const response = await fetch(new URL(url, location.href));
  if (!response.ok) throw new Error(`Cannot load the steps of ${url}: ${response.status}.`);
  return response.json();
}

function remembered() {
  try {
    return JSON.parse(localStorage.getItem("sqlty-playground")) ?? {};
  } catch {
    return {};
  }
}

function remember(change) {
  try {
    localStorage.setItem("sqlty-playground", JSON.stringify({ ...remembered(), ...change }));
  } catch {
    // Private windows may refuse storage: the page works without it.
  }
}

async function share() {
  const hash = await encode(session());
  const url = new URL(location.href);
  url.hash = hash;
  const fits = url.href.length <= MAX_LINK;
  history.replaceState(null, "", fits ? url : url.pathname);
  linked.textContent = fits ? "" : `Too large to share as a link (${url.href.length} characters).`;
  return fits ? url.href : null;
}

function describe(query) {
  const item = document.createElement("li");
  const head = document.createElement("code");
  head.textContent = `${query.line}: ${query.row_type ?? "untyped"}`;
  item.append(head);
  const rows = [
    ...query.columns.map((column) => [
      `${column.name}: ${column.type ?? "?"} (${column.sql_type ?? "?"})`,
      ...explain(column),
    ]),
    ...query.params.map((param) => [
      `param ${param.name ?? param.index}: ${param.type ?? "?"}${param.nullable === false ? " NOT NULL" : ""}`,
    ]),
  ];
  const list = document.createElement("ul");
  list.replaceChildren(
    ...rows.map(([row, ...rules]) => {
      const entry = document.createElement("li");
      entry.textContent = row;
      if (rules.length) {
        const nested = document.createElement("ul");
        nested.replaceChildren(...rules);
        entry.append(nested);
      }
      return entry;
    }),
  );
  item.append(list);
  return item;
}

// What `sqlty check --explain` prints under a column: the inference rules that typed it.
function explain(column) {
  const rules = [column.type_rule, column.nullability_rule].filter(
    (rule, index, all) => rule && all.findIndex((other) => other?.id === rule.id) === index,
  );
  return rules.map((rule) => {
    const parts = [
      column.type_rule?.id === rule.id && "type",
      column.nullability_rule?.id === rule.id && "nullability",
    ].filter(Boolean);
    const entry = document.createElement("li");
    const id = document.createElement("code");
    id.textContent = rule.id;
    entry.append(`${parts.join(" and ")} by `, id, `: ${rule.reasoning} Reference: ${rule.reference}`);
    return entry;
  });
}

let timer;
const later = () => {
  clearTimeout(timer);
  timer = setTimeout(() => {
    refresh();
    share();
  }, 150);
};
for (const model of Object.values(models)) model.onDidChangeContent(later);
dialect.addEventListener("change", later);
for (const tab of files) tab.addEventListener("click", () => open(tab.dataset.file));
for (const tab of tabs) tab.addEventListener("click", () => show(tab.dataset.tab));
for (const button of root.querySelectorAll("[data-zoom]")) {
  button.addEventListener("click", () => zoom(Number(button.dataset.zoom)));
}
for (const button of root.querySelectorAll("[data-step]")) {
  button.addEventListener("click", () => go(step + Number(button.dataset.step)));
}
picker.addEventListener("change", () => go(Number(picker.value)));
root.querySelector("[data-step-reset]").addEventListener("click", () => go(step));
present.addEventListener("click", () => presenting(!root.classList.contains("sqlty-present")));
document.addEventListener("fullscreenchange", () => {
  if (!document.fullscreenElement && root.classList.contains("sqlty-present")) presenting(false);
});
document.addEventListener("keydown", (event) => {
  if (!root.contains(event.target) && !root.classList.contains("sqlty-present")) return;
  const mod = event.ctrlKey || event.metaKey;
  const inEditor = event.target.closest?.(".monaco-editor");
  if (mod && (event.key === "=" || event.key === "+" || event.key === "-")) zoom(event.key === "-" ? -1 : 1);
  else if (mod && event.shiftKey && event.key.toLowerCase() === "f") presenting(!root.classList.contains("sqlty-present"));
  else if (event.key === "Escape" && root.classList.contains("sqlty-present") && !inEditor && event.target.tagName !== "SELECT") {
    presenting(false);
  }
  else if (steps.length && (event.altKey || !inEditor) && (event.key === "PageDown" || event.key === "PageUp")) {
    go(step + (event.key === "PageDown" ? 1 : -1));
  } else return;
  event.preventDefault();
  event.stopPropagation();
}, true);
root.addEventListener("dragover", (event) => event.preventDefault());
root.addEventListener("drop", async (event) => {
  const [file] = event.dataTransfer.files;
  if (!file || !file.name.endsWith(".json")) return;
  event.preventDefault();
  try {
    take(JSON.parse(await file.text()));
    example.value = chosen();
  } catch (error) {
    linked.textContent = `${file.name}: ${error.message}`;
  }
});
resizable(root.querySelector("[data-splitter]"), root.querySelector(".sqlty-console"));
for (const [button, made] of [["fix", "fixed"], ["rewrite", "rewritten"], ["strip", "stripped"]]) {
  root.querySelector(`[data-${button}]`).addEventListener("click", () => {
    refresh();
    const model = models.python;
    if (current[made] === model.getValue()) return;
    open("python");
    editor.pushUndoStop();
    editor.executeEdits("sqlty", [{ range: model.getFullModelRange(), text: current[made] }]);
    editor.pushUndoStop();
  });
}
root.querySelector("[data-copy-link]").addEventListener("click", async () => {
  const link = await share();
  if (!link) return;
  await navigator.clipboard.writeText(link);
  linked.textContent = "Link copied.";
});
window.addEventListener("hashchange", async () => {
  const saved = await decode(location.hash.slice(1));
  if (saved) resume(saved);
});

// A shared session: its steps, then the edits made on its step.
function resume(saved) {
  if (Array.isArray(saved.steps)) begin(saved, saved.step ?? 0);
  restore(saved);
  example.value = chosen();
}

// Drag `handle` to resize `panel`, or focus it and press the arrow keys.
function resizable(handle, panel) {
  const set = (height) => {
    const most = root.clientHeight - 10 * 16;
    const fitted = Math.max(48, Math.min(most, height));
    root.style.setProperty("--sqlty-console", `${fitted}px`);
    remember({ console: fitted });
  };
  if (saved.console) root.style.setProperty("--sqlty-console", `${saved.console}px`);
  handle.addEventListener("pointerdown", (event) => {
    handle.setPointerCapture(event.pointerId);
    const from = event.clientY;
    const height = panel.offsetHeight;
    const move = (moved) => set(height - (moved.clientY - from));
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", () => handle.removeEventListener("pointermove", move), { once: true });
  });
  handle.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    set(panel.offsetHeight + (event.key === "ArrowUp" ? 32 : -32));
  });
}

if (saved.size) root.style.setProperty("--sqlty-font", `${size}px`);
if (shared) resume(shared);
else if (linkedDeck?.error) linked.textContent = String(linkedDeck.error.message ?? linkedDeck.error);
else if (linkedDeck) {
  begin(linkedDeck);
  example.value = chosen();
}
refresh();

async function engine() {
  const url = new URL("sqlty_wasm.wasm", import.meta.url);
  const bytes = await (await fetch(url)).arrayBuffer();
  let ask = await load(bytes);
  return (request) => {
    try {
      return ask(request);
    } catch (error) {
      // A trap leaves the instance unusable: start a new one for the next request.
      load(bytes).then((fresh) => (ask = fresh));
      return { error: String(error) };
    }
  };
}

function loadMonaco() {
  return new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = `${MONACO}/loader.js`;
    script.onerror = reject;
    script.onload = () => {
      window.require.config({ paths: { vs: MONACO } });
      window.require(["vs/editor/editor.main"], () => resolve(window.monaco), reject);
    };
    document.head.append(script);
  });
}
