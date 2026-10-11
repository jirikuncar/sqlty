# Schema inference

With a schema file, sqlty reads column types from `CREATE TABLE` statements,
so queries need no casts. Point it at a schema dump, a migrations snapshot or
a hand-written file.

## Configure a schema

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    total NUMERIC(10, 2) NOT NULL
);
```

Columns, qualified columns and `SELECT *` are typed from the schema:

```python
users = "--sql\nSELECT id, name, created_at FROM users"
totals = "--sql\nSELECT u.name, o.total FROM users u JOIN orders o ON o.user_id = u.id"
everything = "--sql\nSELECT * FROM orders"
# rewritten: users = SQL[tuple[int, str, datetime.datetime]]("--sql\nSELECT id, name, created_at FROM users")
# rewritten: totals = SQL[tuple[str, float]]("--sql\nSELECT u.name, o.total FROM users u JOIN orders o ON o.user_id = u.id")
# rewritten: everything = SQL[tuple[int, int, float]]("--sql\nSELECT * FROM orders")
```

An explicit cast still wins over the schema:

```python
query = "--sql\nSELECT id::TEXT, name FROM users"
# rewritten: query = SQL[tuple[str, str]]("--sql\nSELECT id::TEXT, name FROM users")
```

`sqlty check` warns about `SELECT *` even when it can be typed
([`select-star`](../reference/rules.md#select-star)): the row type changes
whenever the table does. Without a schema, `SELECT *` is an error
([`select-star-no-schema`](../reference/rules.md#select-star-no-schema)).

Nullability is not tracked: `email` above is `str`, not `str | None`.

## Where the schema comes from

`sqlty rewrite` and `sqlty check` use, in order:

1. `--schema PATH` on the command line, relative to the current directory.
2. `schema` in `[tool.sqlty]`, relative to the directory of `pyproject.toml`.

Each file uses the nearest `pyproject.toml` above it that has a `[tool.sqlty]`
table. A schema file that does not exist is reported as a warning and
inference continues without it.

The [magic codec](magic-codec.md) never uses a schema. Stub generation for
`@sqlty` helpers runs at runtime, so there is no `--schema`: it reads `schema`
from `[tool.sqlty]` in the `pyproject.toml` nearest the file of the first
recorded helper (not one per file, as above). A helper can override it with
`schema` in its `[[tool.sqlty.functions]]` entry,
[`[tool.sqlty.schemas]`](../reference/configuration.md#schemas) (schemas
per module or helper) or `@sqlty(schema=...)`; see
[Generated stubs](stubs.md#schemas).

## Dialects

The dialect decides how queries and the schema are parsed. Choose
`postgres` (default), `sqlite` or `mysql` with `dialect` in `[tool.sqlty]` or
`--dialect` on the command line, which overrides `pyproject.toml`.

### SQLite

SQLite has no `::` casts; use `CAST(x AS TYPE)` or a schema.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    score REAL,
    attachment BLOB,
    created_at TIMESTAMP
);
```

```python
notes = "--sql\nSELECT id, title, score, attachment, created_at FROM notes"
stats = "--sql\nSELECT COUNT(*) AS n, MAX(score), CAST(id AS TEXT) FROM notes"
# rewritten: notes = SQL[tuple[int, str, float, bytes, datetime.datetime]]("--sql\nSELECT id, title, score, attachment, created_at FROM notes")
# rewritten: stats = SQL[tuple[int, float, str]]("--sql\nSELECT COUNT(*) AS n, MAX(score), CAST(id AS TEXT) FROM notes")
```

SQLite accepts free-form type names such as `UNSIGNED BIG INT`; sqlty does
not, and skips a table that uses one (see
[Backends](../reference/backends.md#known-gaps)).

### MySQL

MySQL schemas may quote identifiers with backticks and use `UNSIGNED`,
`ENUM`, `SET` and `JSON` columns:

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE `orders` (
    `id` INT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    `total` DECIMAL(10, 2),
    `paid` TINYINT(1),
    `status` ENUM('new', 'paid'),
    `tags` SET('gift', 'rush'),
    `meta` JSON,
    `created_at` DATETIME
);
```

```python
orders = "--sql\nSELECT `id`, total, paid, status, tags, meta, created_at FROM `orders`"
casts = "--sql\nSELECT CAST(total AS SIGNED), CAST(id AS CHAR) FROM orders"
# rewritten: orders = SQL[tuple[int, float, int, str, str, dict, datetime.datetime]]("--sql\nSELECT `id`, total, paid, status, tags, meta, created_at FROM `orders`")
# rewritten: casts = SQL[tuple[int, str]]("--sql\nSELECT CAST(total AS SIGNED), CAST(id AS CHAR) FROM orders")
```

`TINYINT(1)` is an `int`, not a `bool`. See the
[dialect notes](writing-queries.md#dialect-notes) for MySQL-specific types.

!!! warning "MySQL servers reject `--sql` queries"
    The `--sql` marker line is part of the query: `SQL[T]` is a `str`, and
    PyMySQL and mysqlclient send it to the server unchanged. MySQL and
    MariaDB read `--` as a comment only when whitespace follows it, so
    `--sql\nSELECT ...` fails at runtime with a syntax error (1064), even
    though sqlty types it. Until sqlty accepts a MySQL-safe marker, drop the
    marker line before executing, e.g. `cur.execute(query.partition("\n")[2])`.
    Placeholders (`%s`, `%(name)s`) are inferred in every dialect; see
    [Backends](../reference/backends.md#placeholders).

## Example projects

- [`examples/02_schema_inference/`](https://github.com/jirikuncar/sqlty/tree/main/examples/02_schema_inference):
  schema passed with `--schema`.
- [`examples/03_pyproject_config/`](https://github.com/jirikuncar/sqlty/tree/main/examples/03_pyproject_config):
  schema, dialect and rules in `pyproject.toml`.
