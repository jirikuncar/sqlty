// Natural language to SQL in the browser: a small model writes the query and sqlty checks it
// against the schema, sending the problems back until the query is clean. No DOM: the
// playground and scripts/ask_eval.mjs both use it.

const VENDOR = new URL("../vendor/", import.meta.url).href;

export const MODELS = [
  {
    id: "qwen2.5-coder-0.5b",
    label: "Qwen2.5 Coder 0.5B",
    repo: "Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF",
    file: "qwen2.5-coder-0.5b-instruct-q4_k_m.gguf",
    bytes: 491400064,
  },
  {
    id: "qwen3.5-0.8b",
    label: "Qwen3.5 0.8B",
    repo: "ggml-org/Qwen3.5-0.8B-GGUF",
    file: "Qwen3.5-0.8B-Q4_0.gguf",
    bytes: 563036064,
  },
  {
    id: "qwen2.5-coder-1.5b",
    label: "Qwen2.5 Coder 1.5B",
    repo: "Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF",
    file: "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
    bytes: 1117320768,
  },
  {
    id: "qwen3.5-2b",
    label: "Qwen3.5 2B",
    repo: "unsloth/Qwen3.5-2B-GGUF",
    file: "Qwen3.5-2B-Q4_K_M.gguf",
    bytes: 1280835840,
  },
];

export const modelUrl = (model) => `https://huggingface.co/${model.repo}/resolve/main/${model.file}`;

const DIALECTS = { postgres: "PostgreSQL", mysql: "MySQL", sqlite: "SQLite" };
// A bare --sql string is what the model writes: its being unwrapped is no problem.
const UNWRAPPED = "stale-rewrite";

export const HERE = "# <the new query goes here>";

// Prompt variants the eval compares (scripts/ask_eval): `rules` adds the instructions small
// models miss, `examples` two worked questions on another schema, `hints` the columns of the
// query's tables to sqlty's feedback.
export const VARIANTS = {
  base: {},
  rules: { rules: true },
  examples: { examples: true },
  hints: { hints: true },
  all: { rules: true, examples: true, hints: true },
};

const RULES =
  " Select only the columns the question asks for." +
  " Join tables on the columns their REFERENCES name." +
  " Filter on a date or time with a range, such as placed_at >= '2025-01-01' AND placed_at < '2026-01-01'.";

const EXAMPLE_SCHEMA = `CREATE TABLE authors (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE books (
    id INTEGER PRIMARY KEY,
    author_id INTEGER NOT NULL REFERENCES authors (id),
    title TEXT NOT NULL
);

CREATE TABLE loans (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES books (id),
    loaned_at TIMESTAMP NOT NULL
);`;

const EXAMPLES = [
  [
    "Number of books per author name, including authors without books.",
    "SELECT a.name, COUNT(b.id) AS books\nFROM authors AS a\nLEFT JOIN books AS b ON b.author_id = a.id\nGROUP BY a.id, a.name",
  ],
  [
    "Titles of the books loaned in 2024.",
    "SELECT DISTINCT b.title\nFROM books AS b\nJOIN loans AS l ON l.book_id = b.id\nWHERE l.loaned_at >= '2024-01-01' AND l.loaned_at < '2025-01-01'",
  ],
];

export function messages({ schema, dialect, question, source, rules = false, examples = false }) {
  const file = source?.trim()
    ? `\n\nThe Python file it goes in, at the line ${HERE}:\n\n\`\`\`python\n${source.trim()}\n\`\`\``
    : "";
  const shown = examples
    ? EXAMPLES.flatMap(([asked, sql]) => [
        { role: "user", content: `${EXAMPLE_SCHEMA}\n\n${asked}` },
        { role: "assistant", content: `\`\`\`sql\n${sql}\n\`\`\`` },
      ])
    : [];
  return [
    {
      role: "system",
      content:
        `You write one ${DIALECTS[dialect] ?? dialect} SELECT query for the schema the user gives. ` +
        "Use only its tables and columns, and give every computed column an alias." +
        (rules ? RULES : "") +
        " Reply with the query alone in a ```sql block.",
    },
    ...shown,
    { role: "user", content: `${schema.trim()}${file}\n\n${question.trim()}` },
  ];
}

// Columns by table, from CREATE TABLE statements.
export function tables(schema) {
  const found = new Map();
  for (const [, name, body] of schema.matchAll(/CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?"?(\w+)"?\s*\(([\s\S]*?)\n\s*\)\s*;/gi)) {
    const columns = body
      .split("\n")
      .map((line) => line.trim().match(/^"?(\w+)"?\s/)?.[1])
      .filter((column) => column && !/^(PRIMARY|FOREIGN|UNIQUE|CHECK|CONSTRAINT)$/i.test(column));
    found.set(name.toLowerCase(), columns);
  }
  return found;
}

// The file around `line` (1-based, the new query goes after it), at most `limit` characters.
export function around(source, line, limit = 4000) {
  const lines = source.split("\n");
  lines.splice(line, 0, HERE);
  const marked = lines.join("\n");
  if (marked.length <= limit) return marked;
  const at = marked.indexOf(HERE);
  const start = Math.max(0, Math.min(at - limit / 2, marked.length - limit));
  const cut = marked.slice(start, start + limit);
  return start ? cut.slice(cut.indexOf("\n") + 1) : cut;
}

export function extract(text) {
  const fenced = [...text.matchAll(/```(?:sql)?\s*\n?([\s\S]*?)```/gi)].map((found) => found[1]);
  const sql = (fenced.at(-1) ?? text.replace(/^[\s\S]*?(?=\b(?:WITH|SELECT)\b)/i, "")).trim();
  return sql.replace(/;\s*$/, "").trim();
}

export const python = (sql, name = "answer") => `${name} = """--sql\n${sql.replaceAll('"""', '\\"\\"\\"')}"""\n`;

export function problems(engine, { sql, schema, dialect, config = "" }) {
  const answer = engine({ op: "analyze", source: python(sql), schema, dialect, config });
  if (answer.error) return { problems: [answer.error] };
  return {
    problems: answer.diagnostics
      .filter((found) => found.rule !== UNWRAPPED && (found.severity === "error" || found.rule === "unparsed-query"))
      .map((found) => found.message),
    rowType: answer.queries[0]?.row_type ?? null,
    rewritten: answer.rewritten,
  };
}

// Ask `complete` (messages in, text out) for a query, and send sqlty's problems back at most
// `repairs` times. Every attempt is kept, so a caller can show what was fixed.
export async function answer({ complete, engine, schema, dialect, config, question, source, repairs = 2, ...prompt }) {
  const conversation = messages({ schema, dialect, question, source, ...prompt });
  const known = prompt.hints ? tables(schema) : new Map();
  const attempts = [];
  for (let round = 0; round <= repairs; round++) {
    const reply = await complete(conversation);
    const sql = extract(reply);
    const checked = problems(engine, { sql, schema, dialect, config });
    attempts.push({ reply, sql, ...checked });
    if (!checked.problems.length) break;
    const named = new Set(sql.toLowerCase().match(/\w+/g));
    const hints = [...known].filter(([name]) => named.has(name)).map(([name, columns]) => `- ${name}: ${columns.join(", ")}`);
    conversation.push(
      { role: "assistant", content: `\`\`\`sql\n${sql}\n\`\`\`` },
      {
        role: "user",
        content:
          `sqlty found these problems:\n${checked.problems.map((found) => `- ${found}`).join("\n")}\n` +
          (hints.length ? `The columns of the tables it uses:\n${hints.join("\n")}\n` : "") +
          "Fix the query.",
      },
    );
  }
  return { sql: attempts.at(-1).sql, attempts };
}

let library;

export async function wllama({
  wasm = `${VENDOR}wllama/esm/wasm/wllama.wasm`,
  module = `${VENDOR}wllama/esm/index.min.js`,
  compat = { worker: `${VENDOR}wllama-compat/wasm/wllama.js`, wasm: `${VENDOR}wllama-compat/wasm/wllama.wasm` },
} = {}) {
  library ??= import(module).catch((error) => {
    library = undefined;
    throw error;
  });
  const { Wllama } = await library;
  const instance = new Wllama({ default: wasm }, { suppressNativeLog: true });
  instance.setCompat(compat);
  return instance;
}

export async function loadModel(instance, model, { url = modelUrl(model), onProgress, n_threads, thinking = false } = {}) {
  await instance.loadModelFromUrl(url, {
    n_ctx: 4096,
    progressCallback: ({ loaded, total }) => onProgress?.(loaded, total),
    ...(n_threads ? { n_threads } : {}),
  });
  return (conversation) =>
    instance
      .createChatCompletion({
        messages: conversation,
        max_tokens: thinking ? 2048 : 400,
        temperature: 0,
        chat_template_kwargs: { enable_thinking: thinking },
      })
      .then((response) => response.choices[0].message.content ?? "");
}
