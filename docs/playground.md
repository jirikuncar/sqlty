---
description: Type-check, fix and rewrite SQL queries in Python in your browser, with sqlty compiled to WebAssembly.
hide:
  - toc
---

# Playground

Edit the Python source or the schema below: sqlty checks, types and highlights the
queries as you type. It runs in your browser. The engine is compiled to
[WebAssembly](https://webassembly.org/), and no code leaves the page.

<script>
  if (new URLSearchParams(location.search).has("embed")) document.documentElement.classList.add("sqlty-embedded");
</script>

<div id="sqlty-playground" markdown="0">
  <div class="sqlty-toolbar">
    <label><span class="sqlty-label">Example</span>
      <select data-example></select>
    </label>
    <label><span class="sqlty-label">Dialect</span>
      <select data-dialect>
        <option value="postgres">PostgreSQL</option>
        <option value="mysql">MySQL</option>
        <option value="sqlite">SQLite</option>
      </select>
    </label>
    <span class="sqlty-actions" role="toolbar" aria-label="Actions">
      <button type="button" class="sqlty-button sqlty-button--primary" data-fix>Fix all</button>
      <button type="button" class="sqlty-button" data-rewrite>Rewrite</button>
      <button type="button" class="sqlty-button" data-strip>Strip</button>
      <button type="button" class="sqlty-button" data-format>Format</button>
      <button type="button" class="sqlty-button" data-copy-link><span class="sqlty-wide">Copy link</span><span class="sqlty-narrow"></span></button>
      <span class="sqlty-group" role="group" aria-label="Font size">
        <button type="button" class="sqlty-button" data-zoom="-1" title="Smaller (Ctrl+-)" aria-label="Smaller">A&minus;</button>
        <button type="button" class="sqlty-button" data-zoom="1" title="Larger (Ctrl+=)" aria-label="Larger">A+</button>
      </span>
      <button type="button" class="sqlty-button" data-present title="Fill the screen (Ctrl+Shift+F)" aria-pressed="false">Present</button>
      <a class="sqlty-button sqlty-open" data-open target="_blank" rel="noopener">Open in playground</a>
    </span>
    <span class="sqlty-status" data-link-status role="status"></span>
  </div>
  <nav class="sqlty-steps" aria-label="Steps" data-steps hidden>
    <button type="button" class="sqlty-button" data-step="-1" title="Previous step (PageUp)" aria-label="Previous step">&larr;</button>
    <select data-step-select aria-label="Step"></select>
    <button type="button" class="sqlty-button" data-step="1" title="Next step (PageDown)" aria-label="Next step">&rarr;</button>
    <button type="button" class="sqlty-button" data-step-reset title="Undo the edits of this step" aria-label="Reset step">&#8634;<span class="sqlty-label"> Reset</span></button>
  </nav>
  <div class="sqlty-tabs" role="tablist" aria-label="Files">
    <button type="button" role="tab" aria-selected="true" data-file="python">queries.py</button>
    <button type="button" role="tab" aria-selected="false" data-file="schema">schema.sql</button>
    <button type="button" role="tab" aria-selected="false" data-file="config">sqlty.toml</button>
  </div>
  <div class="sqlty-editor" role="tabpanel" aria-label="Editor" data-source></div>
  <div class="sqlty-splitter" role="separator" aria-orientation="horizontal" aria-label="Resize the console" tabindex="0" data-splitter></div>
  <div class="sqlty-tabs" role="tablist" aria-label="Output">
    <button type="button" role="tab" aria-selected="false" data-tab="notes" hidden>Notes</button>
    <button type="button" role="tab" aria-selected="true" data-tab="problems">Problems</button>
    <button type="button" role="tab" aria-selected="false" data-tab="types">Types</button>
    <button type="button" role="tab" aria-selected="false" data-tab="json">JSON</button>
  </div>
  <div class="sqlty-console" role="tabpanel" aria-label="Output">
    <div data-panel="notes" data-notes hidden></div>
    <ul data-panel="problems" data-problems><li>Loading the engine...</li></ul>
    <ul data-panel="types" data-types hidden></ul>
    <div data-panel="json" data-json hidden></div>
  </div>
</div>

<style>
  .md-grid { max-width: 100rem; }
  #sqlty-playground {
    --sqlty-font: 0.68rem; --sqlty-console: clamp(5rem, 30%, 16rem);
    display: flex; flex-direction: column; height: max(32rem, calc(100dvh - 4rem)); scroll-margin-top: 3rem;
  }
  html:has(.sqlty-present) { overflow: hidden; }
  #sqlty-playground.sqlty-present {
    position: fixed; inset: 0; z-index: 10; height: 100dvh; padding: 0.5rem 1rem 1rem;
    background-color: var(--md-default-bg-color);
  }
  html.sqlty-embedded { overflow: hidden; }
  .sqlty-embedded .md-header, .sqlty-embedded .md-tabs, .sqlty-embedded .md-sidebar, .sqlty-embedded .md-footer,
  .sqlty-embedded .md-content__inner > :not(#sqlty-playground) { display: none; }
  .sqlty-embedded #sqlty-playground {
    position: fixed; inset: 0; z-index: 10; height: 100dvh; padding: 0.5rem;
    background-color: var(--md-default-bg-color);
  }
  .sqlty-embedded .sqlty-toolbar { margin-bottom: 0.4rem; }
  .sqlty-embedded .sqlty-toolbar label, .sqlty-embedded [data-rewrite], .sqlty-embedded [data-strip], .sqlty-embedded [data-format],
  .sqlty-embedded [data-copy-link], .sqlty-embedded .sqlty-group, .sqlty-embedded [data-present],
  .sqlty-embedded [data-tab="json"] { display: none; }
  .sqlty-open { display: none; }
  .sqlty-embedded .sqlty-open { display: inline-block; text-decoration: none; }
  .sqlty-toolbar { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 0.5rem 1rem; margin-bottom: 0.75rem; }
  .sqlty-toolbar label { display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.64rem; color: var(--md-default-fg-color--light); }
  .sqlty-actions { display: flex; flex-wrap: wrap; gap: 0.4rem; margin-left: auto; }
  .sqlty-group { display: inline-flex; }
  .sqlty-group .sqlty-button:first-child { border-top-right-radius: 0; border-bottom-right-radius: 0; }
  .sqlty-group .sqlty-button:last-child { border-top-left-radius: 0; border-bottom-left-radius: 0; margin-left: -1px; }
  .sqlty-narrow { display: none; }
  .sqlty-narrow::before { content: "Link"; }
  .sqlty-status { flex-basis: 100%; font-size: 0.64rem; }
  .sqlty-status:empty { display: none; }
  #sqlty-playground select, .sqlty-button {
    font: inherit; font-size: 0.7rem; line-height: 1.4; padding: 0.3rem 0.7rem; white-space: nowrap;
    color: var(--md-default-fg-color); background-color: var(--md-code-bg-color);
    border: 1px solid var(--md-default-fg-color--lighter); border-radius: 0.2rem;
  }
  #sqlty-playground select {
    min-width: 9rem; cursor: pointer; appearance: none; padding-right: 1.8rem;
    background-image: linear-gradient(45deg, transparent 50%, currentColor 50%), linear-gradient(135deg, currentColor 50%, transparent 50%);
    background-position: right 0.9rem center, right 0.6rem center; background-size: 0.3rem 0.3rem; background-repeat: no-repeat;
  }
  .sqlty-button { cursor: pointer; }
  .sqlty-button:hover, .sqlty-button:focus-visible, #sqlty-playground select:focus-visible, .sqlty-button[aria-pressed="true"] {
    border-color: var(--md-accent-fg-color); color: var(--md-accent-fg-color);
  }
  .sqlty-button--primary, .sqlty-button--primary:hover, .sqlty-button--primary:focus-visible {
    color: var(--md-primary-bg-color); background-color: var(--md-primary-fg-color); border-color: var(--md-primary-fg-color);
  }
  .sqlty-button:disabled { opacity: 0.4; cursor: default; pointer-events: none; }
  .sqlty-steps { display: flex; align-items: center; gap: 0.4rem; margin-bottom: 0.5rem; }
  .sqlty-steps[hidden] { display: none; }
  #sqlty-playground .sqlty-steps select { flex: 0 1 24rem; min-width: 0; font-weight: 700; text-overflow: ellipsis; }
  .sqlty-tabs { display: flex; overflow-x: auto; border-bottom: 1px solid var(--md-default-fg-color--lightest); flex: none; }
  .sqlty-tabs [role="tab"] {
    font-family: var(--md-code-font-family); font-size: 0.68rem; padding: 0.4rem 0.8rem; white-space: nowrap;
    color: var(--md-default-fg-color--light); background: none; border: 0; border-bottom: 2px solid transparent; cursor: pointer;
  }
  .sqlty-tabs [role="tab"][hidden] { display: none; }
  .sqlty-tabs [role="tab"]:hover { color: var(--md-default-fg-color); }
  .sqlty-tabs [aria-selected="true"] { color: var(--md-default-fg-color); border-bottom-color: var(--md-accent-fg-color); }
  .sqlty-editor { flex: 1 1 0; min-height: 6rem; border: 1px solid var(--md-default-fg-color--lightest); border-top: 0; }
  .sqlty-splitter { flex: none; height: 0.5rem; cursor: row-resize; touch-action: none; position: relative; }
  .sqlty-splitter::after {
    content: ""; position: absolute; left: 50%; top: 50%; width: 2.5rem; height: 0.2rem; transform: translate(-50%, -50%);
    border-radius: 0.1rem; background-color: var(--md-default-fg-color--lighter);
  }
  .sqlty-splitter:hover::after, .sqlty-splitter:focus-visible::after { background-color: var(--md-accent-fg-color); }
  .sqlty-console {
    flex: none; height: var(--sqlty-console); overflow: auto; font-size: var(--sqlty-font);
    background-color: var(--md-code-bg-color); border: 1px solid var(--md-default-fg-color--lightest); border-top: 0;
  }
  #sqlty-playground .sqlty-console > ul, .sqlty-console [data-notes] { margin: 0; padding: 0.5rem 0.8rem; list-style: none; }
  .sqlty-console [data-notes] p { margin: 0 0 0.6em; }
  #sqlty-playground .sqlty-console li { margin-left: 0; }
  #sqlty-playground .sqlty-console li ul { margin: 0.2rem 0 0.4rem 1rem; list-style: none; }
  #sqlty-playground .sqlty-console li { line-height: 1.45; margin-bottom: 0.35em; }
  .sqlty-console [data-problems] li[data-severity] { display: grid; grid-template-columns: 1.2em 1fr auto; column-gap: 0.5em; align-items: baseline; }
  .sqlty-console [data-problems] a { white-space: nowrap; font-size: 0.85em; color: var(--md-default-fg-color--light); }
  .sqlty-console [data-problems] a:hover { color: var(--md-accent-fg-color); }
  .sqlty-console [data-problems] button { font: inherit; color: inherit; text-align: left; padding: 0; background: none; border: 0; cursor: pointer; }
  .sqlty-console [data-problems] button:hover { color: var(--md-accent-fg-color); }
  .sqlty-console code { font-size: 0.92em; padding: 0 0.1em; word-break: normal; overflow-wrap: anywhere; }
  .sqlty-console .sqlty-at { font-family: var(--md-code-font-family); font-size: 0.85em; color: var(--md-default-fg-color--light); }
  #sqlty-playground [data-types] > li { margin-bottom: 0.8em; }
  .sqlty-columns { display: grid; grid-template-columns: max-content max-content 1fr; column-gap: 1.2em; margin: 0.2em 0 0 1.2em; }
  .sqlty-columns > span { color: var(--md-default-fg-color--light); }
  #sqlty-playground .sqlty-columns code { background: none; padding: 0; overflow-wrap: normal; }
  #sqlty-playground .sqlty-columns code.param { color: var(--md-default-fg-color--light); }
  .sqlty-why { grid-column: 1 / -1; padding: 0 0 0.3em 1em; font-size: 0.88em; color: var(--md-default-fg-color--light); }
  .sqlty-why p { margin: 0; }
  .sqlty-why cite { font-style: normal; opacity: 0.8; }
  .sqlty-console [data-severity="error"]::before { content: "\25CF"; color: var(--md-code-hl-number-color, #c62828); }
  .sqlty-console [data-severity="warn"]::before { content: "\25B2"; color: var(--md-code-hl-special-color, #ef6c00); }
  .sqlty-console > div:not([data-notes]) { height: 100%; }
  @media (max-width: 44.984375em) {
    #sqlty-playground .sqlty-toolbar select { width: 100%; min-width: 0; }
    .sqlty-actions { margin-left: 0; }
    #sqlty-playground .sqlty-steps select { flex: 1 1 0; }
    #sqlty-playground { --sqlty-console: 35dvh; height: calc(100dvh - 3rem); }
    #sqlty-playground.sqlty-present { padding: 0.4rem 0.5rem 0.5rem; }
    .sqlty-toolbar { gap: 0.4rem; margin-bottom: 0.4rem; }
    .sqlty-toolbar label { flex: 1 1 0; min-width: 0; }
    .sqlty-label { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    .sqlty-actions { flex: 1 1 100%; flex-wrap: nowrap; overflow-x: auto; scrollbar-width: none; gap: 0.3rem; }
    .sqlty-actions::-webkit-scrollbar { display: none; }
    #sqlty-playground select, .sqlty-button { font-size: 0.62rem; padding: 0.25rem 0.45rem; }
    #sqlty-playground select { padding-right: 1.5rem; background-position: right 0.75rem center, right 0.45rem center; }
    .sqlty-actions > *, .sqlty-group { flex: none; }
    .sqlty-wide { display: none; }
    .sqlty-narrow { display: inline; }
    .sqlty-steps { gap: 0.3rem; margin-bottom: 0.3rem; }
    .sqlty-console [data-problems] li[data-severity] { grid-template-columns: 1.2em 1fr; }
    .sqlty-console [data-problems] a { grid-column: 2; }
    .sqlty-columns { margin-left: 0.6em; column-gap: 0.8em; }
    .sqlty-tabs [role="tab"] { padding: 0.3rem 0.55rem; font-size: 0.62rem; }
  }
</style>

<script type="module" src="assets/playground/playground.js"></script>

The file tabs above the editor switch between the files `sqlty check` reads:

| File | What it holds | On your machine |
|---|---|---|
| `queries.py` | The Python source: diagnostics under the code with their quick fixes, SQL highlighted in query strings, completion and hover in queries | [`sqlty server`](recipes/vscode.md) |
| `schema.sql` | The `CREATE TABLE` statements the queries are typed against | [`schema`](reference/configuration.md#schema) |
| `sqlty.toml` | The `dialect`, `driver`, `import-style`, `param-types`, `preview`, `max-query-variants`, `rules` and `types` of [`[tool.sqlty]`](reference/configuration.md); its `dialect` wins over the select | `sqlty.toml` or `pyproject.toml` |

The console below the editor shows what sqlty makes of them:

| Tab | What it shows | From the command line |
|---|---|---|
| Notes | What a step says about itself, when [steps](#walk-through-steps) are open | |
| Problems | What `sqlty check` reports; click one to go to its line, or its rule to read about it | [`sqlty check`](reference/cli.md) |
| Types | The row type, columns and parameters of each query, and the inference rule behind a column's type or nullability | `sqlty check --explain` |
| JSON | The problems as `sqlty check` prints them for scripts, with the edits of their fixes | [`sqlty check --output-format json`](reference/cli.md) |

## Examples

The **Example** select loads a project of the
[`examples/`](https://github.com/jirikuncar/sqlty/tree/main/examples) directory: its
`queries.py`, `schema.sql`, and the `dialect`, `driver`, `import-style`, `param-types`,
`rules` and `types` of its `pyproject.toml`. Each one is what `sqlty check` reads on your machine, so the page shows
the same problems. **Tour** is the sample the page opens with, and **Tour in steps** walks
through the same ideas one at a time.

Drag the bar between the editor and the console to share the height between them, or
focus it and press the arrow keys. The page remembers the height and the font size.

## Change the source

| Button | Applies to the Python source | From the command line |
|---|---|---|
| **Fix all** | Every quick fix, as `sqlty check --fix` makes them | `sqlty check --fix` |
| **Rewrite** | Each query wrapped in `SQL[T]` | `sqlty rewrite` |
| **Strip** | Each `SQL[T]` wrapper removed | `sqlty strip` |
| **Format** | Each triple-quoted query laid out by [sqruff](guide/formatting.md) | `sqlty format` |

A single fix is a quick fix on its diagnostic: click the light bulb, or press
**Ctrl+.** with the cursor on it. **Ctrl+Z** undoes any of them.

Press **F1** or **Ctrl+Shift+P** (**Cmd+Shift+P** on macOS; Firefox keeps it for a private
window) in the editor, or right-click it, for the same commands as the
[VS Code extension](recipes/vscode.md#commands): **sqlty: Fix All in File**, **sqlty: Wrap
Queries in SQL[T]**, **sqlty: Strip SQL[T] Wrappers**, **sqlty: Format Queries in File**,
**sqlty: Copy Share Link** and **sqlty: Explain Rule**, which opens the page of the rule
under the cursor.

## Offline

The docs work offline once they are open: a service worker keeps every page, and the
playground once you have opened it. In Chrome or Edge, **Install** in the address bar adds
SQLTy as an app; in Safari, **Share** then **Add to Home Screen** (iOS) or **File** then
**Add to Dock** (macOS). The app opens the docs, with the playground among its shortcuts,
and keeps it ready offline from its first start. Steps from `?steps=<url>` need that URL to
be reachable; a file opened or dropped on the page does not.

## Open a docs example

Each Python example of these docs has an **Open in playground** link under it. It opens
the example here with the schema and the `[tool.sqlty]` settings of its page, so
you can edit it and see what changes. An example that needs a helper module, or
a setting that reads files, has no link.

## Share a session

The page keeps the Python source, the schema, the dialect and `sqlty.toml` in its
URL, compressed, as you type. **Copy link** puts that URL on the clipboard: paste it
in a bug report and whoever opens it sees what you see. Nothing is sent to a server,
as the session lives in the part of the URL after `#`.

A session that compresses to more than 8000 characters gets no link: many browsers
and chat apps cut longer URLs.

## Embed

Add `?embed` to a link to show only the editor, the console and **Fix all**, with
**Open in playground** for the other controls. It fills the window, so it fits an
`<iframe>` in a blog post or slides:

```html
<iframe src="https://sqlty.kuncar.dev/playground/?embed#..." width="800" height="500"></iframe>
```

Link previews in social apps show the SQLTy card, not the session: their crawlers never
see the part of the URL after `#`.

## Walk through steps

Steps are sessions in order, each one built on the one before: a talk, a lesson or a
code review, played in the page. The bar above the files moves between them, and
**Reset** brings the current step back to how it started, so a step can be edited
live without losing it.

Load steps from **Open a file...** in the **Example** select, by dropping a `.json` file
on the playground, or with a link: `playground/?steps=<url>` loads the file at that URL,
which must allow [cross-origin requests](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CORS),
as `raw.githubusercontent.com` and `gist.githubusercontent.com` do. Once loaded, the
steps are part of the session, so **Copy link** shares them while they fit in a link.

```json
{
  "title": "Nullable joins",
  "steps": [
    {
      "title": "A LEFT JOIN",
      "schema": "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
      "source": "q = \"--sql\\nSELECT name FROM users\"\n",
      "panel": "types",
      "notes": "Each column is typed from the schema."
    },
    { "title": "A typo", "source": "q = \"--sql\\nSELECT nme FROM users\"\n", "panel": "problems" }
  ]
}
```

| Key | What it sets | When it is left out |
|---|---|---|
| `title` | The name of the step in the bar | `Step N` |
| `source`, `schema`, `config`, `dialect` | The files and dialect, as a [session](#share-a-session) holds them | The one of the step before |
| `panel` | The console tab it opens: `notes`, `problems`, `types` or `json` | The tab open before, or **Problems** if that was **Notes** and the step has none |
| `file` | The file tab it opens: `python`, `schema` or `config` | The file open before |
| `notes` | Text for the **Notes** tab; a blank line starts a paragraph | No **Notes** tab |

## Present

**Present** fills the screen with the playground, and **A−** and **A+** change the font
size of the editor and the console, for a projector or a screen share.

| Keys | What they do |
|---|---|
| **Ctrl+Shift+F** | Present, or stop presenting; **Escape** also stops |
| **Ctrl+=**, **Ctrl+-** | A larger or a smaller font |
| **PageDown**, **PageUp** | The next or the previous step, as most presentation remotes send |

On macOS, **Command** works as **Ctrl**.

!!! note "One file"
    The playground checks `queries.py` by itself: row types from classes, helper and
    driver calls and `# sqlty: ignore` comments work as on your machine, but a helper or
    a type imported from another module is not followed. Keys of `sqlty.toml` that name
    files, such as `schema` and `overrides`, are errors here.
