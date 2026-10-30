# Coding agents

sqlty ships [Agent Skills](https://agentskills.io): instructions a coding agent
loads when it writes a query in a sqlty project, reads a diagnostic, or plans a
schema change. Install them through your agent's plugin marketplace, or write
them from the installed `sqlty`.

## Skills

<!-- skills-table -->
| Skill | Loads when the agent | Covers |
|-------|----------------------|--------|
| `sqlty` | writes or edits a `--sql` string, an `SQL[T]` wrapper or `[tool.sqlty]` | The `--sql` marker, the rewrite and check loop, pitfalls; routes to the skills below |
| `sqlty-adopt` | adds sqlty to a project that doesn't use it | `[tool.sqlty]`, the schema from a dump or migrations, converting one module at a time, the first CI gate |
| `sqlty-breaking` | changes a table or writes a migration | `sqlty breaking`, its findings, an expand and contract plan |
| `sqlty-check` | reads a diagnostic, names a row type, checks parameters | `check`, `--fix`, `--explain`, JSON output, `sqlty rule`, suppressions, severities |
| `sqlty-ci` | sets up or repairs pre-commit or CI | the hooks, the GitHub Actions job, promoting rules to errors |
| `sqlty-rewrite` | writes a query, or adds or removes wrappers | `rewrite`, `strip`, `--check` and `--diff`, `SQL[UNTYPED]`, `import-style`, the magic codec |
| `sqlty-server` | sets up sqlty in an editor, or an editor shows nothing or other diagnostics than `check` | `sqlty server`, the VS Code extension and its settings, fix on save, other LSP clients, breaking findings while editing |
<!-- /skills-table -->

Each skill links to this site, not to files of the repository, so it works in
any project. Its examples are tests of this repository, so they stay true to the
engine. A skill tells the agent to trust `sqlty rule NAME` and `--help` over its
own text when the two disagree.

## Claude Code

```text
/plugin marketplace add jirikuncar/sqlty
/plugin install sqlty@sqlty
```

Update with `/plugin marketplace update sqlty`, or from a shell:

```bash
claude plugin marketplace update sqlty
claude plugin update sqlty@sqlty
```

## Codex

```bash
codex plugin marketplace add jirikuncar/sqlty
codex plugin add sqlty@sqlty
```

Update with `codex plugin marketplace upgrade`.

## Cursor

Run `/add-plugin` in the agent chat, or import the `jirikuncar/sqlty` repository
into a team marketplace. Cursor reads `.cursor-plugin/plugin.json`.

## VS Code and GitHub Copilot

Add the marketplace to your VS Code settings, then run `/plugin install` in chat:

```json
{
  "chat.plugins.marketplaces": ["jirikuncar/sqlty"]
}
```

The Copilot CLI uses the same marketplace:

```bash
copilot plugin marketplace add jirikuncar/sqlty
```

## `npx skills`

```bash
npx skills add jirikuncar/sqlty
```

Pass `-a claude-code`, `-a codex`, `-a cursor` and so on to choose the agents.

## PyPI

The installed `sqlty` writes the skills of its own version into the directory
your agent reads skills from. Run it again after you upgrade sqlty:

```bash
sqlty skill --install .claude/skills
```

`sqlty skill --list` lists the skills, and `sqlty skill NAME` prints one, for an
agent that reads a single instructions file, such as `AGENTS.md`:

```bash
sqlty skill >> AGENTS.md
```

!!! note "Marketplaces follow the default branch"
    A marketplace install gets the skills of the repository's default branch,
    which may be ahead of the `sqlty` you have installed. `sqlty skill --install`
    writes the skills of your installed version. Either way, a skill asks the
    agent to run `sqlty version` and to prefer `--help` when they disagree.

## Machine-readable diagnostics

`sqlty check --output-format json` prints the diagnostics as a JSON array, one
object per diagnostic:

```json
[
  {
    "file": "app/queries.py",
    "rule": "narrow-type",
    "message": "SQL[tuple[int]] should be SQL[tuple[float]]",
    "severity": "error",
    "line": 3,
    "start": {"line": 3, "column": 9},
    "end": {"line": 3, "column": 19},
    "fix": [
      {
        "start": {"line": 3, "column": 9},
        "end": {"line": 3, "column": 19},
        "replacement": "tuple[float]"
      }
    ]
  }
]
```

| Key | What it holds |
|-----|---------------|
| `line` | The line the diagnostic is reported on, where its call or string starts. A `# sqlty: ignore` comment on this line silences it. |
| `start`, `end` | What the diagnostic is about: the row type, the query or the name in it, a parameter type or the comment. Lines and columns start at 1, columns count characters, and `end` is the position after the last one. |
| `fix` | The edits `sqlty check --fix` makes for this diagnostic, in the same positions. Empty when it has no fix. |

`line` and `start` differ when a call spans lines: a row type checked against the
query is reported on the call's first line and underlines the type wherever it is.

`--output-format github` prints them as GitHub Actions annotations, which show on
the lines of the pull request's diff. See [CI and pre-commit](ci.md#github-actions).
