---
hide:
  - toc
---

# Playground

Edit the Python source or the schema below: sqlty checks, types and highlights the
queries as you type. It runs in your browser. The engine is compiled to
[WebAssembly](https://webassembly.org/), and no code leaves the page.

<div id="sqlty-playground" markdown="0">
  <div class="sqlty-toolbar">
    <label>Example
      <select data-example></select>
    </label>
    <label>Dialect
      <select data-dialect>
        <option value="postgres">PostgreSQL</option>
        <option value="mysql">MySQL</option>
        <option value="sqlite">SQLite</option>
      </select>
    </label>
    <span class="sqlty-actions">
      <button type="button" class="sqlty-button sqlty-button--primary" data-fix>Fix all</button>
      <button type="button" class="sqlty-button" data-rewrite>Rewrite</button>
      <button type="button" class="sqlty-button" data-strip>Strip</button>
      <button type="button" class="sqlty-button" data-copy-link>Copy link</button>
    </span>
    <span class="sqlty-status" data-link-status role="status"></span>
  </div>
  <div class="sqlty-tabs" role="tablist" aria-label="Files">
    <button type="button" role="tab" aria-selected="true" data-file="python">queries.py</button>
    <button type="button" role="tab" aria-selected="false" data-file="schema">schema.sql</button>
    <button type="button" role="tab" aria-selected="false" data-file="config">sqlty.toml</button>
  </div>
  <div class="sqlty-editor" role="tabpanel" aria-label="Editor" data-source></div>
  <div class="sqlty-tabs" role="tablist" aria-label="Output">
    <button type="button" role="tab" aria-selected="true" data-tab="problems">Problems</button>
    <button type="button" role="tab" aria-selected="false" data-tab="types">Types</button>
    <button type="button" role="tab" aria-selected="false" data-tab="rewritten">sqlty rewrite</button>
    <button type="button" role="tab" aria-selected="false" data-tab="json">JSON</button>
  </div>
  <div class="sqlty-console" role="tabpanel" aria-label="Output">
    <ul data-panel="problems" data-problems><li>Loading the engine...</li></ul>
    <ul data-panel="types" data-types hidden></ul>
    <div data-panel="rewritten" data-rewritten hidden></div>
    <div data-panel="json" data-json hidden></div>
  </div>
</div>

<style>
  .md-grid { max-width: 100rem; }
  #sqlty-playground { display: flex; flex-direction: column; }
  .sqlty-toolbar { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 0.5rem 1rem; margin-bottom: 0.75rem; }
  .sqlty-toolbar label { display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.64rem; color: var(--md-default-fg-color--light); }
  .sqlty-actions { display: flex; flex-wrap: wrap; gap: 0.4rem; margin-left: auto; }
  .sqlty-status { flex-basis: 100%; font-size: 0.64rem; }
  .sqlty-status:empty { display: none; }
  .sqlty-toolbar select, .sqlty-button {
    font: inherit; font-size: 0.7rem; line-height: 1.4; padding: 0.3rem 0.7rem; white-space: nowrap;
    color: var(--md-default-fg-color); background-color: var(--md-code-bg-color);
    border: 1px solid var(--md-default-fg-color--lighter); border-radius: 0.2rem;
  }
  .sqlty-toolbar select { min-width: 9rem; cursor: pointer; }
  .sqlty-button { cursor: pointer; }
  .sqlty-button:hover, .sqlty-button:focus-visible, .sqlty-toolbar select:focus-visible {
    border-color: var(--md-accent-fg-color); color: var(--md-accent-fg-color);
  }
  .sqlty-button--primary, .sqlty-button--primary:hover, .sqlty-button--primary:focus-visible {
    color: var(--md-primary-bg-color); background-color: var(--md-primary-fg-color); border-color: var(--md-primary-fg-color);
  }
  .sqlty-tabs { display: flex; overflow-x: auto; border-bottom: 1px solid var(--md-default-fg-color--lightest); }
  .sqlty-tabs [role="tab"] {
    font-family: var(--md-code-font-family); font-size: 0.68rem; padding: 0.4rem 0.8rem; white-space: nowrap;
    color: var(--md-default-fg-color--light); background: none; border: 0; border-bottom: 2px solid transparent; cursor: pointer;
  }
  .sqlty-tabs [role="tab"]:hover { color: var(--md-default-fg-color); }
  .sqlty-tabs [aria-selected="true"] { color: var(--md-default-fg-color); border-bottom-color: var(--md-accent-fg-color); }
  .sqlty-editor { height: min(60vh, 40rem); border: 1px solid var(--md-default-fg-color--lightest); border-top: 0; }
  .sqlty-console { height: 16rem; overflow: auto; background-color: var(--md-code-bg-color); border: 1px solid var(--md-default-fg-color--lightest); border-top: 0; }
  #sqlty-playground .sqlty-console > ul { margin: 0; padding: 0.5rem 0.8rem; font-size: 0.68rem; list-style: none; }
  #sqlty-playground .sqlty-console li { margin-left: 0; }
  #sqlty-playground .sqlty-console li ul { margin: 0.2rem 0 0.4rem 1rem; list-style: none; }
  .sqlty-console [data-problems] button { font: inherit; color: inherit; text-align: left; padding: 0.1rem 0; background: none; border: 0; cursor: pointer; }
  .sqlty-console [data-problems] button:hover { color: var(--md-accent-fg-color); }
  .sqlty-console > div { height: 100%; }
  @media (max-width: 44.984375em) {
    .sqlty-toolbar label { flex: 1 1 8rem; }
    .sqlty-toolbar select { width: 100%; min-width: 0; }
    .sqlty-actions { margin-left: 0; }
    .sqlty-editor { height: 55vh; }
    .sqlty-console { height: 45vh; }
  }
</style>

<script type="module" src="assets/playground/playground.js"></script>

The file tabs above the editor switch between the files `sqlty check` reads:

| File | What it holds | On your machine |
|---|---|---|
| `queries.py` | The Python source: diagnostics under the code with their quick fixes, SQL highlighted in query strings, completion and hover in queries | [`sqlty server`](recipes/vscode.md) |
| `schema.sql` | The `CREATE TABLE` statements the queries are typed against | [`schema`](reference/configuration.md#schema) |
| `sqlty.toml` | The `dialect`, `import-style`, `param-types` and `rules` of [`[tool.sqlty]`](reference/configuration.md); its `dialect` wins over the select | `sqlty.toml` or `pyproject.toml` |

The console below the editor shows what sqlty makes of them:

| Tab | What it shows | From the command line |
|---|---|---|
| Problems | What `sqlty check` reports; click one to go to its line | [`sqlty check`](reference/cli.md) |
| Types | The row type, columns and parameters of each query, and the inference rule behind a column's type or nullability | `sqlty check --explain` |
| `sqlty rewrite` | The source with each query wrapped in `SQL[T]` | [`sqlty rewrite`](reference/cli.md) |
| JSON | The problems as `sqlty check` prints them for scripts, with the edits of their fixes | [`sqlty check --output-format json`](reference/cli.md) |

## Examples

The **Example** select loads a project of the
[`examples/`](https://github.com/jirikuncar/sqlty/tree/main/examples) directory: its
`queries.py`, `schema.sql`, and the `dialect`, `import-style`, `param-types` and `rules` of its
`pyproject.toml`. Each one is what `sqlty check` reads on your machine, so the page shows
the same problems. **Tour** is the sample the page opens with.

## Change the source

| Button | Applies to the Python source | From the command line |
|---|---|---|
| **Fix all** | Every quick fix, as `sqlty check --fix` makes them | `sqlty check --fix` |
| **Rewrite** | Each query wrapped in `SQL[T]`, as the `sqlty rewrite` tab shows | `sqlty rewrite` |
| **Strip** | Each `SQL[T]` wrapper removed | `sqlty strip` |

A single fix is a quick fix on its diagnostic: click the light bulb, or press
**Ctrl+.** with the cursor on it. **Ctrl+Z** undoes any of them.

## Share a session

The page keeps the Python source, the schema, the dialect and `sqlty.toml` in its
URL, compressed, as you type. **Copy link** puts that URL on the clipboard: paste it
in a bug report and whoever opens it sees what you see. Nothing is sent to a server,
as the session lives in the part of the URL after `#`.

A session that compresses to more than 8000 characters gets no link: many browsers
and chat apps cut longer URLs.

!!! note "Row types from classes need Python"
    The playground reads `tuple[...]` row types. A `TypedDict`, dataclass or Pydantic
    model is resolved by the Python half of sqlty, which does not run in the browser,
    so `sqlty check` can report more on your machine. Keys of `sqlty.toml` that name
    files, such as `schema` and `overrides`, are errors here.
