---
max_turns: 20
allowed_tools: [Read, Write, Edit, Glob, Grep, Skill, Bash]
---

The `USER` query in `app/queries.py` returns a bare tuple. Give its rows a name: a `TypedDict` called `User`.
