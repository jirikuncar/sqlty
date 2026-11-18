# Configuration

```console
$ sqlty check q.py
? 1
q.py:1: [unknown-column] column `nope` does not exist in `users`
Found 1 error(s)

$ sqlty check --isolated q.py
$ sqlty check --isolated --schema schema.sql q.py
? 1
q.py:1: [unknown-column] column `nope` does not exist in `users`
Found 1 error(s)

$ sqlty check --config-file other.toml q.py
$ sqlty check -c 'rules.unknown-column = "warn"' q.py
q.py:1: [unknown-column] column `nope` does not exist in `users`
Found 1 warning(s)

$ sqlty check --dialect sqlite q.py
? 1
q.py:1: [unknown-column] column `nope` does not exist in `users`
Found 1 error(s)

$ sqlty check --dialect nosuch q.py
? 2
error: invalid value 'nosuch' for '--dialect <DIALECT>': unknown dialect "nosuch"; expected one of: postgres, mysql, sqlite, clickhouse, generic, ansi, bigquery, databricks, duckdb, hive, mssql, oracle, redshift, snowflake, spark, sparksql, teradata

For more information, try '--help'.

$ sqlty check --config-file other.toml --isolated q.py
? 2
error: the argument '--config-file <PATH>' cannot be used with '--isolated'

Usage: sqlty check --config-file <PATH> <PATHS>...

For more information, try '--help'.

$ sqlty check -c 'rules.unknown-column = "loud"' q.py
? 2
sqlty: error: --config: invalid severity 'loud' for rule 'unknown-column' in [tool.sqlty.rules]; expected one of: error, warn, ignore

$ sqlty check bad
? 2
sqlty: error: [CWD]/bad/sqlty.toml: unknown key 'shema' in [tool.sqlty] (did you mean 'schema'?); expected one of: dialect, driver, schema, queries, query-format, rules, overrides, import-style, untyped, json-types, param-types, preview, max-query-variants, coerce-row-types, functions, types, format-command, extend, databases, exclude, extend-exclude, respect-gitignore

```
