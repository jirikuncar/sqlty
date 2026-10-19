# Breaking: rule configuration

`sqlty breaking` reports the diagnostics a query gets against the other schema but not
against its own. It turns on the name rules (`unknown-table`, `unknown-column`,
`ambiguous-column`, `unknown-function`, `unknown-type`, `insert-missing-column`) when
the project leaves them unset, so a dropped name is reported even where `check` would
not look. A rule the project configures keeps its configuration: a project ignores a
rule for a reason, such as functions of an extension the schema does not declare, and
`breaking` must not overrule it. Severities in the CLI output and exit code are covered
by `tests/e2e/test_breaking_e2e.py`.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, bio TEXT);
CREATE FUNCTION slug(id INTEGER) RETURNS TEXT AS 'SELECT id::text' LANGUAGE sql STRICT;
```

## An unset name rule is turned on

Nothing is configured, so the dropped function is reported.

`against.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, bio TEXT);
```

```python
# error: [unknown-function] "slug"
slugs = "--sql\nSELECT slug(id) FROM users"
```

## A rule the project ignores stays ignored

<!-- mdtest:xfail breaking forces an ignored name rule back to error -->

The project ignores `unknown-function`; `breaking` reports nothing for it.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-function = "ignore"
```

`against.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, bio TEXT);
```

```python
# error:
slugs = "--sql\nSELECT slug(id) FROM users"
```

## An ignored rule does not hide other rules

<!-- mdtest:xfail breaking forces an ignored name rule back to error -->

Ignoring `unknown-function` leaves `unknown-column` on.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-function = "ignore"
```

`against.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```python
# error: [unknown-column] "column `bio` does not exist in `users`"
bios = "--sql\nSELECT slug(id), bio FROM users"
```

## A diagnostic the query already gets is not repeated

A function neither schema declares, such as one of an extension, is unknown on both
sides, so it is not a breaking change even with the rule turned on.

`against.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, bio TEXT);
CREATE FUNCTION slug(id INTEGER) RETURNS TEXT AS 'SELECT id::text' LANGUAGE sql STRICT;
```

```python
# error:
shapes = "--sql\nSELECT st_asgeojson(bio) FROM users"
```

## An inline suppression holds

```toml
[tool.sqlty]
schema = "schema.sql"
```

`against.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, bio TEXT);
```

```python
# error:
slugs = "--sql\nSELECT slug(id) FROM users"  # sqlty: ignore[unknown-function]
```
