import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setImmediate } from "node:timers/promises";
import { beforeEach, test } from "node:test";

import { loadEngine, startEngine } from "../../engine";
import {
  calls,
  CodeActionKind,
  Diagnostic,
  DiagnosticSeverity,
  document,
  fire,
  languages,
  output,
  Position,
  providers,
  Range,
  Uri,
  workspace,
} from "./vscode";

const fixture = (name: string) => {
  const path = resolve("fixture", name);
  return document(path, readFileSync(path, "utf8"), name.endsWith(".py") ? "python" : "sql");
};

/** The engine started on `documents`, once their first check is done. */
async function started(...documents: ReturnType<typeof document>[]) {
  workspace.textDocuments = documents;
  const disposable = await startEngine(Uri.file(resolve(".")) as never, output() as never);
  await setImmediate();
  return { disposable, collection: languages.collections.at(-1)! };
}

beforeEach(() => {
  calls.length = 0;
});

test("diagnostics follow open, changed, closed files and project edits", async () => {
  const q = fixture("q.py");
  const { disposable, collection } = await started(q, fixture("schema.sql"));
  const [found] = collection.entries.get(q.uri.toString())!;
  assert.equal(found.message, "SQL[tuple[int]] should be SQL[tuple[float | None]]");
  assert.equal(found.severity, DiagnosticSeverity.Error);
  assert.deepEqual(found.code, {
    value: "narrow-type",
    target: found.code && typeof found.code === "object" ? found.code.target : undefined,
  });
  assert.equal(collection.entries.size, 1);

  await fire("close", q);
  assert.equal(collection.entries.size, 0);
  await fire("open", q);
  await setImmediate();
  assert.equal(collection.entries.size, 1);
  await fire("close", q);
  await fire("change", { document: q });
  await setImmediate();
  assert.equal(collection.entries.size, 1);
  for (const kind of ["change", "create", "delete"]) {
    collection.clear();
    await fire(`watcher.${kind} **/*.{toml,sql}`);
    await setImmediate();
    assert.equal(collection.entries.size, 1);
  }
  disposable.dispose();
  assert.equal(providers.hover.length, 0);
});

test("a broken project or engine answer is logged, and checks nothing", async () => {
  const broken = mkdtempSync(join(tmpdir(), "sqlty-engine-"));
  writeFileSync(join(broken, "sqlty.toml"), "schema = [");
  const style = mkdtempSync(join(tmpdir(), "sqlty-engine-"));
  writeFileSync(join(style, "sqlty.toml"), "import-style = 'bogus'");
  const a = document(join(broken, "a.py"), "x = 1");
  const b = document(join(style, "b.py"), "x = 1");
  const { disposable, collection } = await started(a, b);
  assert.deepEqual(collection.entries.get(a.uri.toString()), []);
  assert.deepEqual(collection.entries.get(b.uri.toString()), []);
  assert.deepEqual(
    calls.map(([name, message]) => [name, String(message).split(":")[0]]),
    [
      ["output.error", join(broken, "sqlty.toml")],
      ["output.error", "Unsupported import-style"],
    ],
  );
  const [completion] = providers.completion;
  assert.equal(await completion.provideCompletionItems(a, new Position(0, 0)), undefined);
  disposable.dispose();
});

test("completion, hover and quick fixes come from the engine", async () => {
  const q = fixture("q.py");
  const complete = fixture("complete.py");
  const { disposable, collection } = await started(q);
  const [completion] = providers.completion;
  const items = await completion.provideCompletionItems(complete, complete.positionAt(21));
  assert.deepEqual(
    items.map((item: { label: string; kind: number; detail: string; sortText: string }) => [
      item.label,
      item.kind,
      item.detail,
      item.sortText,
    ]),
    [
      ["id", 4, "u", "0000"],
      ["name", 4, "u", "0001"],
      ["score", 4, "u", "0002"],
    ],
  );
  assert.equal(await completion.provideCompletionItems(q, new Position(0, 0)), undefined);

  const [hover] = providers.hover;
  const shown = await hover.provideHover(q, q.positionAt(70));
  assert.match(shown.contents.value, /^Table `public\.users`/);
  const { start, end } = shown.range;
  assert.equal(q.getText().slice(q.offsetAt(start), q.offsetAt(end)), "users");
  assert.equal(await hover.provideHover(q, new Position(0, 0)), undefined);

  const [quickFix] = providers.codeActions;
  const found = collection.entries.get(q.uri.toString())!;
  const other = new Diagnostic(new Range(0, 0, 0, 1), "m", DiagnosticSeverity.Warning);
  const [action] = quickFix.provideCodeActions(q, undefined, { diagnostics: [...found, other] });
  assert.equal(action.title, "Fix narrow-type");
  assert.deepEqual(action.edit.replaced[0][2], "tuple[float | None]");
  disposable.dispose();
});

test("whole-file actions offer only the kinds asked for that change the file", async () => {
  const wrap = fixture("wrap.py");
  const { disposable } = await started();
  const [, whole] = providers.codeActions;
  const titles = async (only?: CodeActionKind) =>
    (await whole.provideCodeActions(wrap, undefined, { only })).map(
      (action: { title: string }) => action.title,
    );
  assert.deepEqual(await titles(), []);
  assert.deepEqual(await titles(CodeActionKind.QuickFix), []);
  assert.deepEqual(await titles(CodeActionKind.Empty.append("source")), [
    "sqlty: fix all",
    "sqlty: rewrite",
  ]);
  assert.deepEqual(await titles(CodeActionKind.Empty.append("source.format.sqlty")), []);
  const broken = document(join(mkdtempSync(join(tmpdir(), "sqlty-engine-")), "c.py"), "x = 1");
  writeFileSync(join(broken.uri.fsPath, "..", "sqlty.toml"), "schema = [");
  assert.deepEqual(
    await whole.provideCodeActions(broken, undefined, {
      only: CodeActionKind.Empty.append("source"),
    }),
    [],
  );
  disposable.dispose();
});

test("a panic is an internal-error, and a new instance answers the next request", async () => {
  const engine = await loadEngine(Uri.file(resolve(".")) as never);
  const request = { op: "analyze", source: "q = 1", dialect: "postgres" };
  const crashed = engine({ ...request, panic: "boom" });
  const [found, ...rest] = crashed.diagnostics;
  assert.deepEqual(rest, []);
  assert.equal(found.rule, "internal-error");
  assert.match(
    found.message,
    /^sqlty crashed checking this file: boom; please report it at https:\/\/github\.com\/jirikuncar\/sqlty\/issues\/new\?template=bug_report\.yml&title=Crash%3A%20boom&output=boom&version=/,
  );
  assert.equal(engine({ ...request, panic: "again" }).diagnostics[0].rule, "internal-error");
  assert.deepEqual(engine(request).diagnostics, []);
});
