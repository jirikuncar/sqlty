// Natural language to SQL in the browser (docs/assets/playground/ask.js), with a scripted model
// and engine: `node --test tests/ask.test.mjs`.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  HERE,
  MODELS,
  VARIANTS,
  answer,
  around,
  extract,
  loadModel,
  messages,
  modelUrl,
  problems,
  python,
  supported,
  tables,
  wllama,
} from "../docs/assets/playground/ask.js";

const schema = `CREATE TABLE IF NOT EXISTS "users" (
    id INTEGER PRIMARY KEY,
    "name" TEXT NOT NULL,
    CONSTRAINT named UNIQUE (name)
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id)
);`;

test("models are read from Hugging Face", () => {
  assert.equal(
    modelUrl(MODELS[1]),
    "https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF/resolve/main/qwen2.5-coder-0.5b-instruct-q4_k_m.gguf",
  );
  assert.deepEqual(VARIANTS.all, { rules: true, examples: true, hints: true });
});

test("the prompt names the dialect and holds the schema, the file and the question", () => {
  const [system, user] = messages({ schema, dialect: "sqlite", question: " Who? ", source: " x = 1 " });
  assert.match(system.content, /^You write one SQLite SELECT query/);
  assert.doesNotMatch(system.content, /REFERENCES name/);
  assert.equal(user.content, `${schema}\n\nThe Python file it goes in, at the line ${HERE}:\n\n\`\`\`python\nx = 1\n\`\`\`\n\nWho?`);
});

test("rules and worked examples are added on request", () => {
  const found = messages({ schema, dialect: "duckdb", question: "Who?", source: " ", rules: true, examples: true });
  assert.match(found[0].content, /^You write one duckdb SELECT .* REFERENCES name/);
  assert.deepEqual(
    found.map((message) => message.role),
    ["system", "user", "assistant", "user", "assistant", "user"],
  );
  assert.equal(found.at(-1).content, `${schema}\n\nWho?`);
});

test("tables lists the columns of each CREATE TABLE", () => {
  assert.deepEqual([...tables(schema)], [
    ["users", ["id", "name"]],
    ["orders", ["id", "user_id"]],
  ]);
});

test("around marks the line and keeps the file near it within the limit", () => {
  assert.equal(around("a\nb", 1), `a\n${HERE}\nb`);
  const lines = Array.from({ length: 40 }, (_, i) => `line ${i}`).join("\n");
  const start = around(lines, 0, 60);
  assert.ok(start.startsWith(HERE) && start.length <= 60, start);
  const middle = around(lines, 20, 60);
  assert.ok(middle.includes(HERE) && middle.startsWith("line ") && middle.length <= 60, middle);
});

test("extract takes the last fenced block, else the text from its first query", () => {
  assert.equal(extract("```sql\nSELECT 1;\n```\nor\n```\nSELECT 2;\n```"), "SELECT 2");
  assert.equal(extract("Here it is: WITH x AS (SELECT 1) SELECT * FROM x;"), "WITH x AS (SELECT 1) SELECT * FROM x");
});

test("python wraps the query in a marked triple-quoted string", () => {
  assert.equal(python('SELECT """'), 'answer = """--sql\nSELECT \\"\\"\\""""\n');
});

test("problems keeps errors and unparsed queries, not the bare-string rewrite", () => {
  const calls = [];
  const engine = (request) => {
    calls.push(request);
    return {
      diagnostics: [
        { rule: "stale-rewrite", severity: "error", message: "wrap it" },
        { rule: "unknown-column", severity: "error", message: "no nam" },
        { rule: "unparsed-query", severity: "warning", message: "cannot parse" },
        { rule: "narrow-type", severity: "warning", message: "narrower" },
      ],
      queries: [],
      rewritten: "r",
    };
  };
  const found = problems(engine, { sql: "SELECT nam FROM users", schema, dialect: "postgres" });
  assert.deepEqual(found, { problems: ["no nam", "cannot parse"], rowType: null, rewritten: "r" });
  assert.equal(calls[0].config, "");
  assert.deepEqual(problems(() => ({ error: "bad dialect" }), { sql: "", schema, dialect: "x" }), {
    problems: ["bad dialect"],
  });
});

test("answer sends sqlty's problems back until the query is clean", async () => {
  const replies = ["```sql\nSELECT nam FROM users\n```", "SELECT name FROM users"];
  const seen = [];
  const complete = async (conversation) => {
    seen.push(conversation.map((message) => message.content));
    return replies.shift();
  };
  const engine = ({ source }) =>
    source.includes("nam FROM")
      ? { diagnostics: [{ rule: "unknown-column", severity: "error", message: "no `nam`" }], queries: [] }
      : { diagnostics: [], queries: [{ row_type: "tuple[str]" }], rewritten: "ok" };
  const found = await answer({ complete, engine, schema, dialect: "postgres", question: "Names?", hints: true });
  assert.equal(found.sql, "SELECT name FROM users");
  assert.deepEqual(
    found.attempts.map((attempt) => attempt.problems),
    [["no `nam`"], []],
  );
  assert.equal(found.attempts[1].rowType, "tuple[str]");
  assert.equal(
    seen[1].at(-1),
    "sqlty found these problems:\n- no `nam`\nThe columns of the tables it uses:\n- users: id, name\nFix the query.",
  );
});

test("answer stops after its repairs, without hints unless asked", async () => {
  const seen = [];
  const complete = async (conversation) => {
    seen.push(conversation.at(-1).content);
    return "SELECT nope FROM users";
  };
  const engine = () => ({ diagnostics: [{ rule: "unknown-column", severity: "error", message: "no nope" }], queries: [] });
  const found = await answer({ complete, engine, schema, dialect: "postgres", question: "?", repairs: 1 });
  assert.equal(found.attempts.length, 2);
  assert.equal(seen[1], "sqlty found these problems:\n- no nope\nFix the query.");
});

const fake = (body) => `data:text/javascript,${encodeURIComponent(body)}`;

test("supported needs WebAssembly exceptions and SIMD, and OPFS", (t) => {
  const real = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  t.after(() => Object.defineProperty(globalThis, "navigator", real));
  const browser = (storage) => Object.defineProperty(globalThis, "navigator", { value: { storage }, configurable: true });
  browser(undefined);
  assert.equal(supported(), false);
  browser({ getDirectory: async () => ({}) });
  assert.equal(supported(), true);
  t.mock.method(WebAssembly, "validate", () => false);
  assert.equal(supported(), false);
});

test("wllama loads its module once, and again after a failed load", async () => {
  await assert.rejects(wllama({ module: fake("throw new Error('offline')") }), /offline/);
  const module = fake(`
    export class Wllama {
      constructor(paths, options) { Object.assign(this, { paths, options }); }
      setCompat(compat) { this.compat = compat; }
    }`);
  const instance = await wllama({ module, wasm: "w.wasm", compat: { worker: "c.js" } });
  assert.deepEqual(instance.paths, { default: "w.wasm" });
  assert.deepEqual(instance.options, { suppressNativeLog: true });
  assert.deepEqual(instance.compat, { worker: "c.js" });
  const again = await wllama({ module: fake("throw new Error('not read')") });
  assert.match(again.paths.default, /vendor\/wllama\/esm\/wasm\/wllama\.wasm$/);
  assert.match(again.compat.worker, /vendor\/wllama-compat\/wasm\/wllama\.js$/);
});

test("loadModel reports progress and completes chats with the model", async () => {
  const calls = [];
  const instance = {
    async loadModelFromUrl(url, options) {
      calls.push({ url, options });
      options.progressCallback({ loaded: 1, total: 2 });
    },
    async createChatCompletion(request) {
      calls.push(request);
      return { choices: [{ message: { content: request.max_tokens === 400 ? "SELECT 1" : null } }] };
    },
  };
  const progress = [];
  const complete = await loadModel(instance, MODELS[0], { onProgress: (...seen) => progress.push(seen), n_threads: 2 });
  assert.equal(calls[0].url, modelUrl(MODELS[0]));
  assert.equal(calls[0].options.n_threads, 2);
  assert.deepEqual(progress, [[1, 2]]);
  assert.equal(await complete([{ role: "user", content: "?" }]), "SELECT 1");
  assert.deepEqual(calls[1].chat_template_kwargs, { enable_thinking: false });

  const thinking = await loadModel(instance, MODELS[0], { url: "m.gguf", thinking: true });
  assert.equal(calls[2].url, "m.gguf");
  assert.equal("n_threads" in calls[2].options, false);
  calls[2].options.progressCallback({ loaded: 1, total: 1 });
  assert.equal(await thinking([]), "");
  assert.equal(calls[3].max_tokens, 2048);
});
