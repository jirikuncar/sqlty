# Coding agents

The repository ships an [Agent Skill](https://docs.claude.com/en/docs/agents-and-tools/agent-skills/overview)
in `skills/sqlty/`: instructions an agent loads when it writes or edits a query
in a sqlty project, names a row type, or checks a schema change. Copy it into
the directory your agent reads skills from:

```bash
git clone --depth 1 https://github.com/jirikuncar/sqlty.git /tmp/sqlty
mkdir -p .claude/skills
cp -r /tmp/sqlty/skills/sqlty .claude/skills/
```

The skill tells the agent to:

- start each query with `--sql` and a newline, then let `sqlty rewrite` write
  `SQL[T]` instead of guessing `T`;
- name row types with a `TypedDict` or dataclass and let `sqlty check` compare
  them with the query;
- run `sqlty check`, with `--explain` to see which rule typed a column;
- run `sqlty breaking --against origin/main` before a migration ships.

Its examples are tests of this repository, so they stay true to the engine.
