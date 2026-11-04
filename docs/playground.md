# Playground

Edit the Python source or the schema below: sqlty checks, types and highlights the
queries as you type. It runs in your browser. The engine is compiled to
[WebAssembly](https://webassembly.org/), and no code leaves the page.

<div id="sqlty-playground" markdown="0">
  <div class="sqlty-playground-toolbar">
    <label>Dialect
      <select data-dialect>
        <option value="postgres">PostgreSQL</option>
        <option value="mysql">MySQL</option>
        <option value="sqlite">SQLite</option>
      </select>
    </label>
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
  </div>
</div>

<style>
  .sqlty-playground-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
  .sqlty-playground-editor { height: 24rem; border: 1px solid var(--md-default-fg-color--lightest); }
  .sqlty-playground-output { max-height: 16rem; overflow: auto; font-size: 0.75rem; }
  @media (max-width: 60em) { .sqlty-playground-grid { grid-template-columns: 1fr; } }
</style>

<script type="module" src="../assets/playground/playground.js"></script>

| Panel | What it shows | From the command line |
|---|---|---|
| Python | Diagnostics under the code, SQL highlighted in query strings, completion and hover in queries | [`sqlty server`](recipes/vscode.md) |
| Types | The row type, columns and parameters of each query | `sqlty check --explain` |
| Problems | What `sqlty check` reports | [`sqlty check`](reference/cli.md) |
| `sqlty rewrite` | The source with each query wrapped in `SQL[T]` | [`sqlty rewrite`](reference/cli.md) |

!!! note "Row types from classes need Python"
    The playground reads `tuple[...]` row types. A `TypedDict`, dataclass or Pydantic
    model is resolved by the Python half of sqlty, which does not run in the browser,
    so `sqlty check` can report more on your machine. Configuration, such as
    `import-style` and rule severities, uses the defaults.
