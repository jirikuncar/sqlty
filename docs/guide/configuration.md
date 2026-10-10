# Configuration

sqlty reads `[tool.sqlty]` from the nearest `pyproject.toml` above the first
path given on the command line. CLI flags override it. Runnable project:
[`examples/03_pyproject_config/`](../../examples/03_pyproject_config/).

`pyproject.toml`:

```toml
[tool.sqlty]
dialect = "postgres"         # postgres (default), mysql, sqlite
schema = "schema.sql"        # CREATE TABLE statements, relative to pyproject.toml
import-style = "from sqlty import SQL"
untyped = "skip"             # or "sentinel": wrap untypeable queries as SQL[UNTYPED]
format-command = "ruff format --stdin-filename {filename} -"  # for generated .pyi stubs

[tool.sqlty.rules]
select-star = "ignore"
column-name-mismatch = "error"
```

| Key | CLI flag | Guide |
|-----|----------|-------|
| `dialect` | `--dialect` | [below](#dialects) |
| `schema` | `--schema` | [schema inference](schema-inference.md) |
| `import-style` | | [import style](import-style.md) |
| `untyped` | `--untyped` | [untyped queries](untyped.md) |
| `rules` | | [below](#rule-severity), [rules](../rules.md) |

## Dialects

The dialect selects the SQL parser, so each dialect's cast syntax works.

### PostgreSQL

```sql
SELECT id::INTEGER, created_at::TIMESTAMPTZ FROM users
-- inferred: tuple[int, datetime.datetime]
```

### MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(id AS SIGNED), CAST(name AS CHAR) FROM users
-- inferred: tuple[int, str]
```

### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```sql
SELECT CAST(id AS INTEGER), CAST(score AS REAL) FROM users
-- inferred: tuple[int, float]
```

## Rule severity

Each [rule](../rules.md) is `"error"`, `"warn"` or `"ignore"`.
`sqlty check` exits 1 only on errors.

### Defaults

```python
from sqlty import SQL

# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
```

### Overridden

```toml
[tool.sqlty.rules]
select-star-no-schema = "ignore"
```

```python
from sqlty import SQL

q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
# error:
```

## Pre-commit

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty
        name: sqlty
        entry: bash -c 'uv run sqlty rewrite . && uv run sqlty check .'
        language: system
        pass_filenames: false
        types: [python]
```

Config is resolved once, from the first path: run one command per project
directory when they have their own `pyproject.toml`.
