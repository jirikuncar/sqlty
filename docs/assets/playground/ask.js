// Natural language to SQL in the browser: a small model writes the query and sqlty checks it
// against the schema, sending the problems back until the query is clean. No DOM: the
// playground and scripts/ask_eval.mjs both use it.

const WLLAMA = "https://cdn.jsdelivr.net/npm/@wllama/wllama@3.13.0/esm";

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

export function messages({ schema, dialect, question }) {
  return [
    {
      role: "system",
      content:
        `You write one ${DIALECTS[dialect] ?? dialect} SELECT query for the schema the user gives. ` +
        "Use only its tables and columns, and give every computed column an alias. " +
        "Reply with the query alone in a ```sql block.",
    },
    { role: "user", content: `${schema.trim()}\n\n${question.trim()}` },
  ];
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
export async function answer({ complete, engine, schema, dialect, config, question, repairs = 2 }) {
  const conversation = messages({ schema, dialect, question });
  const attempts = [];
  for (let round = 0; round <= repairs; round++) {
    const reply = await complete(conversation);
    const sql = extract(reply);
    const checked = problems(engine, { sql, schema, dialect, config });
    attempts.push({ reply, sql, ...checked });
    if (!checked.problems.length) break;
    conversation.push(
      { role: "assistant", content: `\`\`\`sql\n${sql}\n\`\`\`` },
      { role: "user", content: `sqlty found these problems:\n${checked.problems.map((found) => `- ${found}`).join("\n")}\nFix the query.` },
    );
  }
  return { sql: attempts.at(-1).sql, attempts };
}

let library;

export async function wllama({ wasm = `${WLLAMA}/wasm/wllama.wasm`, module = `${WLLAMA}/index.min.js` } = {}) {
  library ??= import(module).catch((error) => {
    library = undefined;
    throw error;
  });
  const { Wllama } = await library;
  return new Wllama({ default: wasm }, { suppressNativeLog: true });
}

export async function loadModel(instance, model, { url = modelUrl(model), onProgress, n_threads } = {}) {
  await instance.loadModelFromUrl(url, {
    n_ctx: 4096,
    progressCallback: ({ loaded, total }) => onProgress?.(loaded, total),
    ...(n_threads ? { n_threads } : {}),
  });
  return (conversation) =>
    instance
      .createChatCompletion({
        messages: conversation,
        max_tokens: 400,
        temperature: 0,
        chat_template_kwargs: { enable_thinking: false },
      })
      .then((response) => response.choices[0].message.content ?? "");
}
