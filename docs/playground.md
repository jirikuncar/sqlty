# Playground

Edit the Python source or the schema below: sqlty checks, types and highlights the
queries as you type. It runs in your browser. The engine is compiled to
[WebAssembly](https://webassembly.org/), and no code leaves the page.

<div id="sqlty-playground" markdown="0">
  <div class="sqlty-playground-toolbar">
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
    <button type="button" class="md-button" data-fix>Fix all</button>
    <button type="button" class="md-button" data-rewrite>Rewrite</button>
    <button type="button" class="md-button" data-strip>Strip</button>
    <button type="button" class="md-button" data-copy-link>Copy link</button>
    <span data-link-status role="status"></span>
  </div>
  <h3>Python</h3>
  <div class="sqlty-playground-editor" data-source></div>
  <div class="sqlty-playground-grid">
    <section>
      <h3>Types</h3>
      <ul class="sqlty-playground-output" data-types><li>Loading the engine...</li></ul>
    </section>
    <section>
      <h3>Problems</h3>
      <ul class="sqlty-playground-output" data-problems></ul>
    </section>
    <section>
      <h3><code>schema.sql</code></h3>
      <div class="sqlty-playground-editor" data-schema></div>
    </section>
    <section>
      <h3><code>sqlty rewrite</code></h3>
      <div class="sqlty-playground-editor" data-rewritten></div>
    </section>
    <section>
      <h3><code>sqlty check --output-format json</code></h3>
      <div class="sqlty-playground-editor" data-json></div>
    </section>
    <section>
      <h3><code>sqlty.toml</code></h3>
      <div class="sqlty-playground-editor sqlty-playground-config" data-config></div>
    </section>
  </div>
</div>

<style>
  .sqlty-playground-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
  .sqlty-playground-editor { height: 24rem; border: 1px solid var(--md-default-fg-color--lightest); }
  .sqlty-playground-config { height: 12rem; }
  .sqlty-playground-toolbar { display: flex; align-items: center; gap: 1rem; }
  .sqlty-playground-output { max-height: 16rem; overflow: auto; font-size: 0.75rem; }
  @media (max-width: 60em) { .sqlty-playground-grid { grid-template-columns: 1fr; } }
</style>

<script type="module" src="assets/playground/playground.js"></script>

| Panel | What it shows | From the command line |
|---|---|---|
| Python | Diagnostics under the code with their quick fixes, SQL highlighted in query strings, completion and hover in queries | [`sqlty server`](recipes/vscode.md) |
| Types | The row type, columns and parameters of each query, and the inference rule behind a column's type or nullability | `sqlty check --explain` |
| Problems | What `sqlty check` reports | [`sqlty check`](reference/cli.md) |
| `sqlty rewrite` | The source with each query wrapped in `SQL[T]` | [`sqlty rewrite`](reference/cli.md) |
| `sqlty check --output-format json` | The problems as `sqlty check` prints them for scripts, with the edits of their fixes | [`sqlty check --output-format json`](reference/cli.md) |
| `sqlty.toml` | The `dialect`, `import-style`, `param-types` and `rules` of [`[tool.sqlty]`](reference/configuration.md); its `dialect` wins over the select | `sqlty.toml` or `pyproject.toml` |

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
| **Rewrite** | Each query wrapped in `SQL[T]`, as the `sqlty rewrite` panel shows | `sqlty rewrite` |
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
