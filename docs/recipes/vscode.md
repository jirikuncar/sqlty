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
**sqlty: Restart Server**. Saving `pyproject.toml` or a `.sql` file reloads the
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

## Completion

Inside a query string, the editor completes table names after `FROM` and `JOIN`, and the
columns of the tables in scope after `alias.` and in `SELECT`, `WHERE`, `ON`,
`GROUP BY` and `ORDER BY`. Each column shows its table or alias, and its type and
nullability once selected. Typing `.` opens the list.

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

## Highlighting

A string marked `--sql` or `-- sql` is highlighted as SQL: keywords, strings, numbers,
comments, operators and placeholders. Other strings stay Python strings, and so do
f-strings, raw strings and bytes:

```python
q = "--sql\nSELECT name FROM users WHERE id = %(id)s"  # SQL
plain = "SELECT name FROM users"  # a Python string
```

The extension adds a TextMate grammar to Python, so highlighting needs no server. A
grammar sees the text of one string only, so a string checked without a marker stays
a Python string: the query of an annotated helper or a `[[tool.sqlty.functions]]`
function, and `SQL[T]("SELECT ...")`. A query split across adjacent literals is
highlighted in its first literal only.

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
| `backward` | The queries of the baseline against your schema | When the project loads, and when you save a schema or `pyproject.toml` |

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

!!! warning "The extension does not run in Restricted Mode"
    The server comes from the folder's own environment, so a folder you have not trusted
    would choose what runs. Trust the folder to turn the extension on.

## Other editors

`sqlty server` runs in any LSP client: see [Neovim, Vim and Helix](editors.md) for their
setup, the server's options and SQL highlighting.
