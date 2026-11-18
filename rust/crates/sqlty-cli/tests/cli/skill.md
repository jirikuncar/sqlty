# Agent skills

```console
$ sqlty skill --list
sqlty           Write, type and check raw SQL in Python with sqlty
sqlty-adopt     Introduce sqlty into an existing Python project with raw SQL queries, one module at a time
sqlty-breaking  Check that a schema change or migration doesn't break the queries running during a rolling deploy, with sqlty breaking, and plan an expand and contract migration
sqlty-check     Run sqlty check and act on its diagnostics, name the row type of a query, and check query parameters, including the typed parameters of SQL[R, P]
sqlty-ci        Set up or repair sqlty in pre-commit and CI
sqlty-rewrite   Wrap --sql query strings as SQL[T] with sqlty rewrite, update or remove the wrappers, and keep them in sync after editing a query
sqlty-server    Run sqlty in an editor with sqlty server, the language server, and the VS Code extension

$ sqlty skill sqlty-ci
---
name: sqlty-ci
...

$ sqlty skill sqlty-cd
? 2
Unknown skill 'sqlty-cd'; run `sqlty skill --list` for the list

$ sqlty skill --install skills
Wrote skills/sqlty/SKILL.md
Wrote skills/sqlty-adopt/SKILL.md
Wrote skills/sqlty-breaking/SKILL.md
Wrote skills/sqlty-check/SKILL.md
Wrote skills/sqlty-ci/SKILL.md
Wrote skills/sqlty-rewrite/SKILL.md
Wrote skills/sqlty-server/SKILL.md

```
