---
description: How to use the sqlty VS Code extension to see SQL diagnostics in Python as you type, with quick fixes, column completion and SQL highlighting.
---

# VS Code

The sqlty extension shows what `sqlty check` reports while you type, each diagnostic
under the name, row type or query it is about, with the fix of `sqlty check --fix` as a
quick fix.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL

# error[45:49]: [unknown-column] "column `nmae` does not exist in `users`"
q = SQL[tuple[int, str]]("--sql\nSELECT id, nmae FROM users")
```

The editor underlines `nmae`, and hovering it shows the message and a link to the
rule's page.

## Install

Install **sqlty** from the
[Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=jirikuncar.sqlty),
or from [Open VSX](https://open-vsx.org/extension/jirikuncar/sqlty) in Cursor, VSCodium and
other editors built on VS Code:

```bash
code --install-extension jirikuncar.sqlty
```

The extension runs `sqlty server` from your project's environment, so add sqlty there:

```bash
uv add --dev sqlty
```

!!! info "The editor and CI run the same sqlty"
    The extension bundles no sqlty. It runs the version your project pins, with your
    `[tool.sqlty]` settings, so it reports what `sqlty check` reports in CI.

## How the server is found

The first `sqlty` of version 0.3.0 or newer runs:

| Order | Where | Set by |
|-------|-------|--------|
| 1 | Each path of `sqlty.path` | You |
| 2 | `python -m sqlty` of the interpreter | `sqlty.interpreter`, else the one selected in the Python extension |
| 3 | `sqlty` on `PATH` | Your shell |

!!! warning "An older sqlty is skipped"
    The extension reads `sqlty --version` and skips a `sqlty` older than it supports,
    even one in `sqlty.path`: an older server would start and quietly miss the settings it
    predates, such as `sqlty.breaking.against`. When every `sqlty` found is older, the error
    names each one, its version and where it was found. Upgrade the project's:
    `uv add --dev 'sqlty>=0.3.0'`.

The extension restarts the server when one of these changes, or when you run
**sqlty: Restart Server**. Saving `sqlty.toml`, `pyproject.toml`, `Cargo.toml` or a `.sql` file reloads the
project's settings and schema without a restart.

## Fix on save

`source.fixAll.sqlty` is `sqlty check --fix` on the file: a stale `SQL[T]` is rewritten,
an inline `--sql` comment is split onto its own line, and so on. Run it on every save:

```json
{
  "[python]": {
    "editor.codeActionsOnSave": {
      "source.fixAll.sqlty": "explicit"
    }
  }
}
```

## Format on save

`source.format.sqlty` is `sqlty format` on the file: it lays out the query strings with
sqruff. It is a code action, not a document formatter, so Ruff or Black still formats the
Python code. Run both on every save:

```json
{
  "[python]": {
    "editor.codeActionsOnSave": {
      "source.fixAll.sqlty": "explicit",
      "source.format.sqlty": "explicit"
    }
  }
}
```

## Completion

Inside a query string, the editor completes table names after `FROM` and `JOIN`, and the
columns of the tables in scope after `alias.` and in `SELECT`, `WHERE`, `ON`,
`GROUP BY` and `ORDER BY`. Each column shows its table or alias, and its type and
nullability once selected. With nothing typed and several tables in scope, columns come
qualified, `o.user_id` and `u.id`, so the inserted name is never ambiguous. Typing `.`
opens the list. After `JOIN`, the tables a foreign
key links to one in scope come first, with the `ON` clause that joins them:
`orders ON orders.user_id = u.id`.

A string completes exactly when `sqlty check` checks it, by the same rules:

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT id, <CURSOR> FROM users")
# completions: id column users
# completions: name column users
```

A string passed to a [query helper](../guide/helpers.md), with no marker:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T]) -> list[T]: ...

fetch_all("SELECT u.<CURSOR> FROM users u", tuple[int, str])
# completions: id column u
# completions: name column u
```

A method whose receiver has an evident class, as in `check`:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def fetch_one[T](self, query: Annotated[LiteralString, SQL], form: type[T]) -> T: ...

db = Db()
db.fetch_one("SELECT name FROM users WHERE <CURSOR> = 1", str)
# completions: id column users
# completions: name column users
```

Adjacent literals are one query, so a table name completes in the last of them:

```python
q = (
    "--sql\n"
    "SELECT count(*) "
    "FROM us<CURSOR>"
)
# completions: users table public
```

A function registered in [`[[tool.sqlty.functions]]`](../guide/helpers.md#helpers-you-cannot-annotate)
completes too. What `check` does not check does not complete: a call to anything else,
an f-string, a variable.

```python
def run(query: str) -> None: ...

run("SELECT <CURSOR> FROM users")
# completions: None
```

!!! note "Other suggestions stay in the list"
    VS Code merges the suggestions of every extension. Inside a query string, sqlty's
    come first, ranked by `sortText`, and the Python extension's word suggestions
    follow: a language server cannot remove the suggestions of another.

Turn it off with `"sqlty.completion.enable": false`; the server then does no completion
work. It needs sqlty 0.4.0 or newer, and with an older one the output channel says so.

## Hover

Hover a word of a query string to see what it is. A column shows its SQL type, whether
it can be NULL and where it comes from; a table or view lists its columns; a word at the
top level of a select item shows the result column, with the inference rule that typed
it, as `sqlty check --explain` does:

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total INTEGER);
```

```python
q = "--sql\nSELECT user_id, SUM(total) AS spe<CURSOR>nt FROM orders GROUP BY user_id"
# hover: Result column `spent`: `int | None` (`BIGINT`)
# hover:
# hover: Typed by `postgres.aggregate.sum-integers`: Summing small integers widens to bigint so the sum fits.
# hover:
# hover: PostgreSQL 17 documentation, 9.21, Table 9.60 General-Purpose Aggregate Functions (sum ( smallint ) → bigint, sum ( integer ) → bigint)
```

VS Code shows the hovers of every extension together, so sqlty's sits next to Pylance's.

## Highlighting

A string marked `--sql` or `-- sql` is highlighted as SQL: keywords, strings, numbers,
comments, operators and placeholders. Other strings stay Python strings, and so do
raw strings and bytes:

```python
q = "--sql\nSELECT name FROM users WHERE id = %(id)s"  # SQL
plain = "SELECT name FROM users"  # a Python string
```

In a t-string or an f-string marked `--sql`, the replacement fields keep the colors of
Python, so you can tell the expressions from the query:

```python
user_id, column = 1, "name"
query = t"--sql\nSELECT name FROM users WHERE id = {user_id}"  # SQL, {user_id} as Python
legacy = f"--sql\nSELECT {column} FROM users"  # SQL, {column} as Python
```

!!! note "A highlighted f-string is still not checked"
    `sqlty check` checks t-strings (`SQLTemplate[T]`), but it skips f-strings and reports
    them as [`fstring-skipped`](../reference/rules.md#fstring-skipped).

In a triple-quoted string, the marker may also sit on the first line that is not blank:

```python
q = """
--sql
SELECT name FROM users
"""
```

The extension adds a TextMate grammar to Python, so highlighting needs no server. A
grammar sees the text of one string only, so a string checked without a marker stays
a Python string: the query of an annotated helper or a `[[tool.sqlty.functions]]`
function, and `SQL[T]("SELECT ...")`. A query split across adjacent literals is
highlighted in its first literal only.

!!! tip "Strings marked for another highlighter"
    [python-string-sql](https://marketplace.visualstudio.com/items?itemName=ptweir.python-string-sql),
    [Inline SQL](https://marketplace.visualstudio.com/items?itemName=qufiwefefwoyn.inline-sql-syntax)
    and other SQL highlighters for Python mark strings with the same `--sql` comment, so sqlty
    highlights and checks them as they are. Keep one highlighter enabled, so one grammar colors
    each string.

## Breaking changes

Set `sqlty.breaking.against` to a baseline, and the editor also shows what
[`sqlty breaking`](../guide/breaking-changes.md) reports against it:

```json
{
  "sqlty.breaking.against": "main"
}
```

| Direction | What is checked | When |
|-----------|-----------------|------|
| `forward` | Your queries against the schema of the baseline | With every check of a file |
| `backward` | The queries of the baseline against your schema | When the project loads, and when you save a schema or a configuration file |

`sqlty.breaking.direction` picks one; the default is `both`. A finding's source is
`sqlty breaking (forward)` or `sqlty breaking (backward)`, and it has no quick fix: the
fix belongs to the migration or the code, depending on which ships first.

!!! note "A moved baseline is read again on reload"
    The server resolves `main` when the project loads. When `main` moves, save a schema
    or run **sqlty: Restart Server**.

## Settings

| Setting | Default | Meaning |
|---------|---------|---------|
| `sqlty.path` | `[]` | `sqlty` executables to try first |
| `sqlty.interpreter` | `[]` | The interpreter whose `sqlty` runs; empty is the one selected in the Python extension |
| `sqlty.breaking.against` | empty | A baseline for `sqlty breaking`: a git revision, a `--save` snapshot or schema files |
| `sqlty.breaking.direction` | `both` | `both`, `forward` or `backward` |
| `sqlty.completion.enable` | `true` | Complete table and column names in query strings |
| `sqlty.trace.server` | `off` | Log the protocol messages in the **sqlty** output channel |

## Restricted Mode

In Restricted Mode, the extension highlights the SQL of strings marked `--sql` and runs
nothing. Diagnostics, quick fixes and completion start once you trust the folder, without a
reload.

!!! warning "The server waits for trust"
    The server comes from the folder's own environment: `sqlty.path`, the selected
    interpreter or `PATH`. A folder you have not trusted would choose what runs.

## VS Code for the Web

On [vscode.dev](https://vscode.dev) and [github.dev](https://github.dev), the browser runs
no processes, so there is no `sqlty server`. The extension runs the engine itself, compiled
to WebAssembly, as the [playground](../playground.md) does: diagnostics with their quick
fixes, completion and hover in the query strings, and SQL highlighting.

[Open the examples on github.dev](https://github.dev/jirikuncar/sqlty/blob/main/examples/07_joins/queries.py)
to try it: install the recommended **SQLTy** extension when VS Code offers it, then open a
`queries.py` of another example. Each example has its own `pyproject.toml`, so each is
checked against its own schema.

!!! note "Needs the next release of the extension"
    vscode.dev and github.dev install the extension from the Marketplace. The engine
    ships in the release after sqlty 0.5.1; before it, the web extension highlights only.

The configuration is found as `sqlty check` finds it: the closest `sqlty.toml`,
`pyproject.toml` with `[tool.sqlty]` or `Cargo.toml` with a sqlty table above each file,
with `extend` applied, so each project of a monorepo gets its own. The engine reads
`dialect`, `schema`, `rules`, `types`, `import-style` and `param-types`.

!!! note "What the web leaves out"
    The checks that read row types from Python source (`column-type-mismatch` of a
    `TypedDict`, ...), the parameters of driver calls (`param-count-mismatch`), helpers
    registered in `functions`, named `databases`, `overrides`, `queries` files, fix-all
    on save and breaking changes need `sqlty server`, on the desktop.

## Other editors

`sqlty server` runs in any LSP client: see [Neovim, Vim, Helix and Emacs](editors.md) for their
setup, the server's options and SQL highlighting.
