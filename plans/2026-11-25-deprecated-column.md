# `deprecated-column`

> **Status:** implemented on branch `deprecated-column` (#1257); delete this plan once it ships.

`[tool.sqlty.deprecated-columns]` marks columns scheduled for removal; the SQL rule
`deprecated-column` (warn) reports each read of one.

```toml
[tool.sqlty.deprecated-columns]
"users.legacy_email" = { use = "email", remove-after = "2027-02-20" }
```

## Design

- Config: a table of `table.column` to a table with optional `use` (string) and
  `remove-after` (string). Validated in `sqlty_python::config`, allowed in a database.
- Carried with `types`: `own_types` adds a `("!table.column", "use\tremove-after")` pair, so
  every host that loads a schema with `types` (CLI, server, snapshot, mdtest) gets it
  without a new field. `TypeOverrides::new` keeps the `!` entries in a `deprecated` map.
- Analyzer: `column_expr` resolves every column a query reads (select list, `WHERE`, `JOIN`,
  `RETURNING`, `CASE`, function arguments, `SET` values). A schema column marked
  deprecated records `deprecated-column` with its span. Writes (`INSERT` column list,
  `UPDATE ... SET col`) never go through `column_expr`.
- `SELECT *` is not a read of a named column; `select-star` covers it.
