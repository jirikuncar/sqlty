// The docs playground's engine (docs/assets/playground/engine.js) on the WebAssembly build:
// `make playground`, then `node --test tests/playground.test.mjs`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { load } from "../docs/assets/playground/engine.js";
import { decode, encode } from "../docs/assets/playground/share.js";
import { fold, unfold } from "../docs/assets/playground/steps.js";

const ask = await load(await readFile(new URL("../docs/assets/playground/sqlty_wasm.wasm", import.meta.url)));
const schema = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);";

test("a query is typed against the schema", () => {
  const source = 'q = "--sql\\nSELECT id, name FROM users"';
  const answer = ask({ op: "analyze", source, schema, dialect: "postgres" });
  assert.equal(answer.queries[0].row_type, "tuple[int, str]");
});

test("a request larger than the memory the module starts with", () => {
  const columns = Array.from({ length: 20000 }, () => "name").join(", ");
  const source = `q = "--sql\\nSELECT ${columns} FROM users"`;
  const answer = ask({ op: "analyze", source, schema, dialect: "sqlite" });
  assert.equal(answer.queries[0].columns.length, 20000);
});

test("a row type, a helper call and an ignore comment are checked", () => {
  const source = [
    "from typing import Annotated, LiteralString, TypedDict",
    "from sqlty import SQL",
    "class User(TypedDict):",
    "    id: int",
    "    nam: str",
    "def fetch[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...",
    'q = SQL[User]("--sql\\nSELECT id, name FROM users")',
    'r = SQL[User]("--sql\\nSELECT id, name FROM users")  # sqlty: ignore',
    'names = fetch("SELECT name FROM users", int)',
    "x = 1  # sqlty: ignore",
    "",
  ].join("\n");
  const answer = ask({ op: "analyze", source, schema, dialect: "postgres" });
  const found = answer.diagnostics.map((d) => [d.line, d.rule]);
  assert.ok(found.some(([line, rule]) => line === 7 && rule !== "unused-ignore"), JSON.stringify(found));
  assert.ok(!found.some(([line]) => line === 8), JSON.stringify(found));
  assert.ok(found.some(([line, rule]) => line === 9 && rule === "column-type-mismatch"), JSON.stringify(found));
  assert.deepEqual(found.filter(([, rule]) => rule === "unused-ignore"), [[10, "unused-ignore"]]);
});

test("an unknown dialect is an error", () => {
  assert.match(ask({ op: "analyze", source: "", dialect: "postgress" }).error, /postgress/);
});

test("a configuration applies its dialect, import style and rules", () => {
  const source = 'q = "--sql\\nSELECT 1 = 1 AS b"';
  const config = 'dialect = "sqlite"\nimport-style = "import sqlty"\n[rules]\nstale-rewrite = "error"';
  const answer = ask({ op: "analyze", source, dialect: "postgres", config });
  assert.equal(answer.queries[0].row_type, "tuple[int]");
  assert.equal(answer.diagnostics[0].severity, "error");
  assert.match(answer.rewritten, /^import sqlty\n/);
});

test("param-types = true makes Rewrite and Fix all write the parameter types", () => {
  const source = 'q = "--sql\\nSELECT name FROM users WHERE id = %s"\n';
  const answer = ask({ op: "analyze", source, schema, dialect: "postgres", config: "param-types = true" });
  const typed = 'q = SQL[tuple[str], tuple[int | None]]("--sql\\nSELECT name FROM users WHERE id = %s")\n';
  assert.equal(answer.rewritten, `from sqlty import SQL\n${typed}`);
  assert.equal(answer.fixed, answer.rewritten);
});

test("format lays out a triple-quoted query with sqruff", () => {
  const source = 'q = """--sql\nselect id,name from users where id=%s"""\n';
  const answer = ask({ op: "format", source, dialect: "postgres" });
  assert.equal(answer.formatted, 'q = """--sql\nselect\n    id,\n    name\nfrom users\nwhere id = %s"""\n');
});

test("a configuration key that needs files is an error", () => {
  const answer = ask({ op: "analyze", source: "", dialect: "postgres", config: 'schema = "schema.sql"' });
  assert.match(answer.error, /not 'schema'/);
});

test("a session survives its share link", async () => {
  const session = { source: "q = 'é'\n".repeat(200), schema, dialect: "mysql", config: "" };
  const hash = await encode(session);
  assert.match(hash, /^[A-Za-z0-9_-]+$/);
  assert.ok(hash.length < JSON.stringify(session).length / 4);
  assert.deepEqual(await decode(hash), session);
  assert.equal(await decode("not-a-session"), null);
});

test("a diagnostic carries its quick fix, and the source what fix and strip make", () => {
  const source = 'q = "--sql\\nSELECT nme FROM users"\n';
  const answer = ask({ op: "analyze", source, schema, dialect: "postgres" });
  const typo = answer.diagnostics.find((found) => found.rule === "unknown-column");
  assert.deepEqual(typo.fix, [{ start: 19, end: 22, replacement: "name" }]);
  assert.equal(answer.fixed, 'from sqlty import SQL\nq = SQL[tuple[str]]("--sql\\nSELECT name FROM users")\n');
  assert.equal(ask({ op: "analyze", source: answer.fixed, schema, dialect: "postgres" }).stripped, source.replace("nme", "name"));
});

test("each example of examples/ checks as sqlty check does", async () => {
  const url = new URL("../docs/assets/playground/examples.json", import.meta.url);
  const examples = JSON.parse(await readFile(url, "utf8"));
  assert.ok(examples.length >= 5);
  for (const example of examples) {
    const answer = ask({ op: "analyze", ...example });
    assert.equal(answer.error, undefined, example.name);
    assert.deepEqual(answer.diagnostics.filter((found) => found.severity === "error"), [], example.name);
    assert.equal(answer.rewritten, example.source, example.name);
  }
});

test("a column names the inference rule that typed it, as --explain does", () => {
  const source = 'q = "--sql\\nSELECT SUM(id) AS total FROM users"';
  const [column] = ask({ op: "analyze", source, schema, dialect: "postgres" }).queries[0].columns;
  assert.equal(column.type_rule.id, "postgres.aggregate.sum-integers");
});

test("a step keeps the files of the step before that it leaves out", () => {
  const [first, second] = unfold({ steps: [{ source: "a", schema: "s" }, { title: "Two", source: "b", panel: "types" }] });
  assert.deepEqual([first.title, first.source, first.schema], ["Step 1", "a", "s"]);
  assert.deepEqual([second.title, second.source, second.schema, second.panel], ["Two", "b", "s", "types"]);
  assert.deepEqual(unfold(fold([first, second])), [first, second]);
  assert.deepEqual(fold([first, second]).steps[1], { title: "Two", source: "b", panel: "types" });
});

test("a deck without steps is an error", () => {
  assert.throws(() => unfold({ steps: [] }), /steps/);
  assert.throws(() => unfold({ steps: [{ source: 1 }] }), /Step 1/);
});

test("each step of the docs' Tour in steps is typed", async () => {
  const deck = JSON.parse(await readFile(new URL("../docs/assets/playground/steps.json", import.meta.url), "utf8"));
  for (const step of unfold(deck)) {
    const answer = ask({ op: "analyze", ...step });
    assert.ok(!answer.error && answer.queries.length, step.title);
  }
});
