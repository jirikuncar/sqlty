---
max_turns: 10
allowed_tools: [Read, Write, Edit, Glob, Grep, Skill, Bash]
---

Add `display_name(name: str | None, email: str) -> str` to `app/format.py`: the name when there is one, else the part of the email before the `@`.
