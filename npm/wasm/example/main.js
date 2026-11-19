import { python, pythonLanguage } from "@codemirror/lang-python";
import { sql } from "@codemirror/lang-sql";
import { forceLinting, linter, lintGutter } from "@codemirror/lint";
import { EditorView, hoverTooltip } from "@codemirror/view";
import { load } from "@sqlty/wasm";
import wasm from "@sqlty/wasm/sqlty_wasm.wasm?url";
import { basicSetup } from "codemirror";

const SCHEMA = `CREATE TABLE users (
  id integer PRIMARY KEY,
  name text NOT NULL,
  email text,
  active boolean NOT NULL DEFAULT true
);
`;

const SOURCE = `from sqlty import SQL

# Hover a column, complete one, and fix both mistakes.
active = SQL("""--sql
SELECT id, nmae, email FROM users WHERE active
""")

by_id = SQL[tuple[int]]("--sql\\nSELECT name FROM users WHERE id = %(id)s")
`;

const KINDS = { table: "class", view: "interface", cte: "variable", column: "property" };

const ask = await load(wasm);

const request = (state, op, more) => ({
  op,
  source: state.doc.toString(),
  schema: schema.state.doc.toString(),
  dialect: "postgres",
  ...more,
});

const check = linter(
  (view) => {
    const answer = ask(request(view.state, "analyze"));
    if ("error" in answer) {
      return [{ from: 0, to: 0, severity: "error", message: answer.error }];
    }
    showTypes(answer.queries);
    return answer.diagnostics.map((found) => ({
      from: found.start,
      to: found.end,
      severity: found.severity === "error" ? "error" : "warning",
      source: found.rule,
      message: found.message,
      actions: found.fix.length
        ? [{ name: "Fix", apply: (view) => view.dispatch({ changes: found.fix.map(change) }) }]
        : [],
    }));
  },
  { delay: 100 },
);

const change = (edit) => ({ from: edit.start, to: edit.end, insert: edit.replacement });

const complete = (context) => {
  const answer = ask(request(context.state, "complete", { offset: context.pos }));
  if (!answer || "error" in answer) return null;
  return {
    from: answer.start,
    to: answer.end,
    options: answer.items.map((item, index) => ({
      label: item.label,
      detail: item.detail,
      type: KINDS[item.kind],
      boost: -index,
    })),
  };
};

const hover = hoverTooltip((view, pos) => {
  const answer = ask(request(view.state, "hover", { offset: pos }));
  if (!answer || "error" in answer) return null;
  return {
    pos: answer.start,
    end: answer.end,
    create() {
      const dom = document.createElement("div");
      dom.className = "hover";
      dom.textContent = answer.contents.replaceAll("`", "");
      return { dom };
    },
  };
});

function showTypes(queries) {
  document.querySelector("#types").replaceChildren(
    ...queries.map((query) => {
      const item = document.createElement("li");
      item.textContent = `line ${query.line}: ${query.row_type ?? "unknown"}`;
      return item;
    }),
  );
}

const schema = new EditorView({
  parent: document.querySelector("#schema"),
  doc: SCHEMA,
  extensions: [
    basicSetup,
    sql(),
    EditorView.updateListener.of((update) => update.docChanged && forceLinting(source)),
  ],
});

const source = new EditorView({
  parent: document.querySelector("#source"),
  doc: SOURCE,
  extensions: [
    basicSetup,
    python(),
    check,
    lintGutter(),
    hover,
    pythonLanguage.data.of({ autocomplete: complete }),
  ],
});
