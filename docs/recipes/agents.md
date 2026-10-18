# Coding agents

sqlty ships an [Agent Skill](https://docs.claude.com/en/docs/agents-and-tools/agent-skills/overview):
instructions an agent loads when it writes or edits a query in a sqlty project,
names a row type, or checks a schema change. The installed `sqlty` writes the
skill of its own version into the directory your agent reads skills from:

```bash
sqlty skill --install .claude/skills
```

`sqlty skill` prints it instead, for an agent that reads one instructions file,
such as `AGENTS.md` or a Cursor rule:

```bash
sqlty skill >> AGENTS.md
```

The skill links to this site, not to files of the repository, so it works in
any project.

The skill tells the agent to:

- start each query with `--sql` and a newline, then let `sqlty rewrite` write
  `SQL[T]` instead of guessing `T`;
- name row types with a `TypedDict` or dataclass and let `sqlty check` compare
  them with the query;
- run `sqlty check`, with `--explain` to see which rule typed a column, or
  `--output-format json` to read the diagnostics as data;
- run `sqlty breaking --against origin/main` before a migration ships.

Its examples are tests of this repository, so they stay true to the engine.

## Machine-readable diagnostics

`sqlty check --output-format json` prints the diagnostics as a JSON array, one
object per diagnostic:

```json
[
  {
    "file": "app/queries.py",
    "rule": "unknown-column",
    "message": "column `nmae` does not exist in `users`",
    "severity": "error",
    "line": 3
  }
]
```

`--output-format github` prints them as GitHub Actions annotations, which show on
the lines of the pull request's diff. See [CI and pre-commit](ci.md#github-actions).
