# pyproject.toml Configuration

SQLTy reads configuration from `[tool.sqlty]` in your `pyproject.toml`.

## Configuration options

```toml
[tool.sqlty]
dialect = "postgres"       # SQL dialect (postgres, mysql, sqlite)
schema = "schema.sql"      # Schema file for type inference
format-command = "ruff format --stdin-filename {filename} -"

[tool.sqlty.rules]
column-count-mismatch = "error"   # Annotation field count != query columns
column-type-mismatch = "error"    # Annotation field type != inferred type
column-name-mismatch = "warn"     # Annotation field name != column name
select-star = "warn"              # SELECT * usage
stale-rewrite = "warn"            # --sql strings not wrapped in SQL[T](...)
```

## Precedence

CLI flags override `pyproject.toml` settings:

```bash
# Uses config from pyproject.toml
sqlty check .

# Overrides dialect from config
sqlty check --dialect sqlite .

# Overrides schema from config
sqlty rewrite --schema other.sql .
```

## Lint rules

Each rule can be set to `"error"`, `"warn"`, or `"ignore"`:

| Rule | Default | Description |
|------|---------|-------------|
| `column-count-mismatch` | error | Annotation field count differs from query columns |
| `column-type-mismatch` | error | Annotation field type differs from inferred type |
| `column-name-mismatch` | warn | Annotation field name differs from column name |
| `select-star` | warn | `SELECT *` usage |
| `select-star-no-schema` | error | `SELECT *` with no schema available |
| `stale-rewrite` | warn | `--sql` strings not wrapped in `SQL[T](...)` |
| `stale-type` | warn | Type annotation doesn't match current inference |
| `inline-sql-comment` | warn | `"--sql SELECT ..."` (query becomes SQL comment) |
