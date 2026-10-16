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
# rewritten: import decimal
# rewritten: users = SQL[tuple[int, str, datetime.datetime | None]]("--sql\nSELECT id, name, created_at FROM users")
# rewritten: totals = SQL[tuple[str, decimal.Decimal]]("--sql\nSELECT u.name, o.total FROM users u JOIN orders o ON o.user_id = u.id")
# rewritten: everything = SQL[tuple[int, int | None, decimal.Decimal]]("--sql\nSELECT * FROM orders")
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

`email` above has no `NOT NULL`, so it is `str | None`; see [Nullability](#nullability).

## Nullability

A column that can be NULL infers `T | None`. sqlty only reports what the schema or the
query proves: a column of unknown nullability (a cast with no schema, an unknown
function) stays `T`.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, nick TEXT);
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total NUMERIC(10, 2));
```

```python
users = "--sql\nSELECT id, name, nick FROM users"
left = "--sql\nSELECT u.id, o.id FROM users u LEFT JOIN orders o ON o.user_id = u.id"
narrowed = "--sql\nSELECT nick FROM users WHERE nick IS NOT NULL"
total = "--sql\nSELECT count(*), max(total) FROM orders"
# rewritten: import decimal
# rewritten: users = SQL[tuple[int, str, str | None]]("--sql\nSELECT id, name, nick FROM users")
# rewritten: left = SQL[tuple[int, int | None]]("--sql\nSELECT u.id, o.id FROM users u LEFT JOIN orders o ON o.user_id = u.id")
# rewritten: narrowed = SQL[tuple[str]]("--sql\nSELECT nick FROM users WHERE nick IS NOT NULL")
# rewritten: total = SQL[tuple[int, decimal.Decimal | None]]("--sql\nSELECT count(*), max(total) FROM orders")
```

What makes a column nullable or not:

- **Schema.** `NOT NULL` and `PRIMARY KEY` columns are non-null; others are nullable. In
  SQLite a `PRIMARY KEY` column is non-null only if it is `INTEGER`, `NOT NULL`, or in a
  `WITHOUT ROWID` or `STRICT` table.
- **Outer joins.** The optional side of `LEFT`, `RIGHT` and `FULL` joins (also nested)
  is nullable.
- **Conditions.** `WHERE`, `HAVING` and the `ON` of inner joins narrow what they prove:
  `IS NOT NULL`, comparisons, `IN`, `BETWEEN`, `LIKE`, a strict function such as
  `lower(x) = 'a'`, and the PostgreSQL `?` operator. `OR` proves only what both sides prove,
  and the right side of `AND`, `OR` and a `CASE` branch is read under what the left side
  established. A non-null column of an outer-joined table in `WHERE` also drops the
  null-extended rows.
- **Expressions.** Operators and most functions are NULL when an argument is.
  `COALESCE` is nullable only if every argument is. `NULLIF`, JSON accessors, array
  subscripts and a `CASE` without `ELSE` can always be NULL.
- **Aggregates.** `COUNT` is never NULL. Other aggregates are nullable without a plain
  `GROUP BY` (empty input), with `FILTER`, or when the argument is.
- **Subqueries.** A scalar subquery is nullable unless it always returns one row.
- **Set operations.** `UNION` is nullable if either side is, `INTERSECT` if both are,
  `EXCEPT` follows its left side.
- **DML.** `RETURNING` reads the schema like a `SELECT`.

[`nullable-mismatch`](../reference/rules.md#nullable-mismatch) reports a row type field
that is not optional for a nullable column;
[`redundant-optional`](../reference/rules.md#redundant-optional) (off by default) reports
the opposite. See [Row types](row-types.md#nullable-columns).

### Upgrading

Before this release, row types were never `T | None`. Existing `SQL[...]` wrappers over
nullable columns are now `stale-type`, and row types that omit `| None` are
`nullable-mismatch` errors. `sqlty check --fix` updates wrappers in place, and
`sqlty rewrite` writes the new types for unwrapped queries. Add `| None` to the flagged
row type fields, filter the NULLs in the query, or lower the rule while you migrate:

```toml
[tool.sqlty.rules]
nullable-mismatch = "warn"
```

## Literal values

When the values of a column are known and few, its type is a `typing.Literal` of them
instead of `str` or `int`, so a type checker can tell `"paid"` from any other string.
Known values come from:

- **Enums.** `CREATE TYPE ... AS ENUM` (PostgreSQL) and MySQL `ENUM('a', 'b')` columns.
- **`CHECK` constraints.** `CHECK (col IN (...))`, `CHECK (col = 'a' OR col = 'b')` and
  `CHECK (col = ANY (ARRAY[...]))` (also with the casts `pg_dump` writes), on a column or
  the table, in PostgreSQL and SQLite, for `TEXT`, `VARCHAR` and integer columns. A
  `CHECK ... AND ...` contributes the conditions it understands, an `OR` only when every
  arm lists values (`col IS NULL` arms add none).
- **Domains.** `CREATE DOMAIN d AS text CHECK (VALUE IN (...))`; a domain over a domain
  keeps the values both allow, and a cast to the domain has them too.
- **Constants and expressions over them.** `SELECT 'paid'` (a bare integer such as
  `SELECT 1` stays `int`), `CASE` whose branches are all
  constants, `COALESCE(col, 'none')`, `UNION` (the union of the values), `VALUES`, and
  `MIN`, `MAX`, `NULLIF`, `LAG` and `FIRST_VALUE` of a column with values.
- **Views, CTEs and subqueries** carry the values of the columns they select.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'refunded')),
    note TEXT
);
```

```python
orders = "--sql\nSELECT status, note, CASE WHEN id > 1 THEN 'late' ELSE 'early' END AS timing FROM orders"
# rewritten: orders = SQL[tuple[typing.Literal['pending', 'paid', 'refunded'], str | None, typing.Literal['late', 'early']]]("--sql\nSELECT status, note, CASE WHEN id > 1 THEN 'late' ELSE 'early' END AS timing FROM orders")
```

A computed value (arithmetic, `upper(col)`, `col || 'x'`, `SUM`) is the plain `str` or
`int` again, so the values are always a superset of what the query can return. A set of
more than 32 values is not tracked: the column is the plain type. Not tracked either: a
`CHECK` in MySQL (before 8.0.16 it is parsed and ignored), `CHAR(n)` columns (the database
pads them), columns with a collation that may equate strings (`COLLATE NOCASE`, any
`COLLATE` but `BINARY`, `C` and `POSIX`) and `citext`, a `CHECK ... NOT VALID`, and a `WHERE col = 'x'` that narrows the column. After
`ALTER TABLE ... DROP CONSTRAINT` the table's columns, and the text and integer columns of
the views declared so far, lose their values, as constraints are not followed by name; a MySQL
`ALTER TABLE ... MODIFY` or `CHANGE` makes the table unknown, and an `ALTER DOMAIN` that changes a constraint leaves the domain's
type unknown.

A field annotated `str` (or `int`) still accepts such a column, and `sqlty check` does
not call that annotation stale. A field annotated with a `Literal` is compared with the
values (also for an item of a `SQL[tuple[...]]` wrapper, where `Literal` annotations are
compared as sets, so quotes and order do not matter): [`literal-mismatch`](../reference/rules.md#literal-mismatch) (an error) reports
values the `Literal` misses, and [`redundant-literal`](../reference/rules.md#redundant-literal)
(off by default) values it lists that the column never takes. See
[Row types](row-types.md#literal-columns).

### Upgrading

`sqlty rewrite` and `sqlty check --fix` now write `typing.Literal[...]` where the values
are known: string constants (`SELECT 'paid'`; `SELECT 1` stays `int`), `CASE` over
constants, `CHECK`ed columns and MySQL `ENUM` columns. Existing wrappers that spell `str` or `int` for
those columns stay valid and are not `stale-type`; an annotation of a different type is.
A row type field that is a `Literal` missing values is now a `literal-mismatch` error; add
the values, annotate `str`, or lower the rule while you migrate:

```toml
[tool.sqlty.rules]
literal-mismatch = "warn"
```

## Views

A `CREATE VIEW` (or `CREATE OR REPLACE VIEW`, `CREATE MATERIALIZED VIEW`) in
the schema is a table whose columns are typed from its query. Views are
analyzed in file order, so a view may read the views before it. A column the
schema cannot type, such as one read from a table that is not in the schema,
stays unknown, as in a CTE.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
CREATE VIEW adults (user_id, user_name) AS SELECT id, name FROM users WHERE age >= 18;
```

```python
adults = "--sql\nSELECT user_id, user_name FROM adults"
# rewritten: adults = SQL[tuple[int, str]]("--sql\nSELECT user_id, user_name FROM adults")
```

## Set operations

`UNION`, `INTERSECT` and `EXCEPT` type each column across all branches, as
PostgreSQL does: `NULL`, placeholders and columns of unknown type yield to the
other branches, numbers promote to the wider type, arrays resolve by their
elements, and types of different categories leave the column unknown. Names come from the leftmost branch.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE events (amount INTEGER NOT NULL);
CREATE TABLE archived_events (amount BIGINT NOT NULL);
```

```python
totals = "--sql\nSELECT sum(amount) FROM (SELECT amount FROM events UNION ALL SELECT amount FROM archived_events) AS u"
# rewritten: import decimal
# rewritten: totals = SQL[tuple[decimal.Decimal | None]]("--sql\nSELECT sum(amount) FROM (SELECT amount FROM events UNION ALL SELECT amount FROM archived_events) AS u")
```

## Domains and enums

A column typed by a `CREATE DOMAIN` gets the domain's base type; domains over
domains resolve to the innermost base type, and a schema-qualified name
(`app.percent`) matches the unqualified one. A column typed by a
`CREATE TYPE ... AS ENUM` is a `typing.Literal` of the enum's labels, and so is
a cast to the enum (`x::order_status`). `sqlty rewrite` adds `import typing` for
it. The SQL type reported for a column is the domain's base type, or the
enum's own (unqualified, upper-case) name.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE DOMAIN percent AS integer CHECK (VALUE BETWEEN 0 AND 100);
CREATE TYPE order_status AS ENUM ('pending', 'paid');
CREATE TABLE orders (id INTEGER PRIMARY KEY, discount percent, status order_status NOT NULL);
```

```python
orders = "--sql\nSELECT discount, status FROM orders"
# rewritten: orders = SQL[tuple[int | None, typing.Literal['pending', 'paid']]]("--sql\nSELECT discount, status FROM orders")
```

A field annotated `str` still accepts an enum column, and a `Literal[...]` field
is checked against the labels; see [Literal values](#literal-values).

## Migrations, schemas and functions

In PostgreSQL the schema is a catalog that the statements build, not a list of
`CREATE TABLE`s, so a migration history works as a schema file:

- `ALTER TABLE`: add, drop and rename columns, change a column type, set or drop
  `NOT NULL`, rename the table. A change to a table reaches the tables that
  inherit from it or are its partitions.
- `ALTER TYPE ... ADD VALUE` and `RENAME VALUE` change an enum's `Literal`.
- `DROP TABLE`, `VIEW`, `TYPE`, `DOMAIN`, `FUNCTION` and `SCHEMA`.
- `PARTITION OF`, `INHERITS` and `LIKE` give a table its parent's columns.
- `CREATE FUNCTION ... RETURNS` types calls to the function: a scalar, a
  `SETOF`, or the columns of `RETURNS TABLE (...)` in `FROM`. Overloads are told
  apart by their number of arguments, and a built-in function of the same name
  wins.
- `pg_catalog` and `information_schema` relations are always known, with their
  PostgreSQL column types and `NOT NULL` columns: `pg_class`, `pg_namespace`,
  `pg_attribute`, `pg_type`, `pg_index`, `pg_constraint`, `pg_enum`, `pg_proc`,
  `pg_attrdef`, `pg_depend`, `pg_description`, `pg_roles`, `pg_settings`,
  `pg_stat_activity`, `pg_tables`, `pg_views`, `pg_indexes`, `pg_sequences`, and
  in `information_schema` `schemata`, `tables`, `views`, `columns`, `routines`,
  `key_column_usage` and `table_constraints`. Types follow psycopg 3, which loads only `oid` specially: `oid` is
  `int`, and `name`, `"char"`, `reg*`, `xid`, `int2vector`, `oidvector`,
  `pg_node_tree` and `aclitem` are `str`. Columns of the system views
  (`pg_settings`, `pg_tables`, ..., all of `information_schema`) are nullable. Other `pg_*` relations are not read (see
  [name diagnostics](name-diagnostics.md)).

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, full_name TEXT, legacy TEXT);
ALTER TABLE users DROP COLUMN legacy;
ALTER TABLE users RENAME COLUMN full_name TO name;
ALTER TABLE users ADD COLUMN status status NOT NULL;

CREATE TYPE status AS ENUM ('active', 'banned');
ALTER TYPE status ADD VALUE 'pending' BEFORE 'active';

CREATE FUNCTION user_count() RETURNS bigint LANGUAGE sql AS 'SELECT count(*) FROM users';
```

The `ALTER TABLE` statements come before the enum they use, and that is fine:
a statement whose dependencies are not defined yet is retried, so files load in
any order.

```python
users = "--sql\nSELECT * FROM users"
total = "--sql\nSELECT user_count()"
# rewritten: users = SQL[tuple[int, str | None, typing.Literal['pending', 'active', 'banned']]]("--sql\nSELECT * FROM users")
# rewritten: total = SQL[tuple[int]]("--sql\nSELECT user_count()")
```

A name written `schema.table` looks in that schema. A bare name follows the
`search_path` only: `pg_catalog`, then the schemas of the `SET search_path` in
effect at the end of the file (`public` when there is none). A table in a schema
off the path is not found by its bare name, even when no other schema defines it:
with `CREATE TABLE app.users (...)` and no `SET search_path`, `FROM users` is an
`unknown-table` and `FROM app.users` works. This holds for tables, views, types
and functions alike.

Names follow PostgreSQL's case rules. An unquoted identifier folds to lower
case; a quoted one keeps its case. With `users(id int)`, `SELECT "ID" FROM users`
names an unknown column, and `"Users"` and `users` are different tables. A column
declared as `"userId"` is read as `"userId"`, not `userId`. `AS Foo` names an
output column `foo`; `AS "Foo"` names it `Foo`. Table aliases and qualifiers are
compared without case. MySQL and SQLite compare every name without case,
whatever the quoting, and have no `search_path`.

Every table has the system columns `ctid` (`tid`), `xmin` and `xmax` (`xid`),
`cmin` and `cmax` (`cid`) and `tableoid` (`oid`). Drivers return `tid`, `xid`
and `cid` as `str`, and `oid` as `int`. A user table has no `oid` column since
PostgreSQL 12 unless it declares one, so `SELECT oid FROM users` is an
`unknown-column`. A built-in function may be written `pg_catalog.lower(x)`.

As in PostgreSQL, `pg_catalog` is searched first, so a user table named like a
system relation (`CREATE TABLE pg_class (...)`) is reached as `public.pg_class`;
a bare `pg_class` is the system one. Other `pg_*` names, such as `pg_orders`,
are ordinary tables in every dialect.

sqlty does not guess where it cannot follow. A table changed by a statement it
cannot read (for instance `ALTER TABLE ... SET SCHEMA`), or built from a table
the schema never defines (`LIKE`, `INHERITS`, `PARTITION OF`), has an unknown
shape: its columns get no type, and `SELECT *` over it is left alone.
Actions that cannot change columns (constraints, storage options, ownership,
triggers, partitions, tablespaces) never do this. `sqlty check` warns about each
table or type it had to leave unknown.
`CREATE TABLE ... AS`, `ALTER VIEW`, `DROP ... CASCADE` of dependent views and
`ALTER SCHEMA ... RENAME` are not followed. Views are analyzed against the
tables as they are where the view is created. MySQL and SQLite read `CREATE`
statements only.

## Where the schema comes from

`sqlty rewrite` and `sqlty check` use, in order:

1. `--schema PATH` on the command line, relative to the current directory.
2. `schema` in `[tool.sqlty]`, relative to the directory of `pyproject.toml`.

Each file uses the nearest `pyproject.toml` above it that has a `[tool.sqlty]`
table. A schema file that does not exist is reported as a warning and
inference continues without it.

The [magic codec](magic-codec.md) never uses a schema.

## Dialects

The dialect decides how queries and the schema are parsed. Choose
`postgres` (default), `sqlite` or `mysql` with `dialect` in `[tool.sqlty]` or
`--dialect` on the command line, which overrides `pyproject.toml`. Other
[dialects](../reference/engine.md#dialects) can be set in `pyproject.toml`.

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
# rewritten: notes = SQL[tuple[int, str, float | None, bytes | None, datetime.datetime | None]]("--sql\nSELECT id, title, score, attachment, created_at FROM notes")
# rewritten: stats = SQL[tuple[int, float | None, str]]("--sql\nSELECT COUNT(*) AS n, MAX(score), CAST(id AS TEXT) FROM notes")
```

#### Free-form type names

SQLite accepts any type name, `UNSIGNED BIG INT` or `VARYING CHARACTER(255)`
included, and derives the column's
[type affinity](https://www.sqlite.org/datatype3.html#determination_of_column_affinity)
from it. sqlty does the same for a name it has no mapping for: one containing
`INT` is `int`; `CHAR`, `CLOB` or `TEXT` is `str`; `BLOB` is `bytes`; `REAL`,
`FLOA` or `DOUB` is `float`; any other is `NUMERIC` (`float`). A column with no
type, or `ANY`, can hold any value and is not typed.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE legacy (id UNSIGNED BIG INT NOT NULL, code VARYING CHARACTER(10), price MONEYISH, raw);
```

```python
legacy = "--sql\nSELECT id, code, price FROM legacy"
# rewritten: legacy = SQL[tuple[int, str | None, float | None]]("--sql\nSELECT id, code, price FROM legacy")
```

### MySQL

MySQL schemas may quote identifiers with backticks and use `UNSIGNED`,
`ENUM`, `SET` and `JSON` columns. `SET` has no known Python type, so a query
that selects one is not typed:

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
orders = "-- sql\nSELECT `id`, total, paid, status, meta, created_at FROM `orders`"
with_set = "-- sql\nSELECT `id`, tags FROM `orders`"
casts = "-- sql\nSELECT CAST(total AS SIGNED), CAST(id AS CHAR) FROM orders"
# rewritten: import decimal
# rewritten: orders = SQL[tuple[int, decimal.Decimal | None, int | None, typing.Literal['new', 'paid'] | None, typing.Any, datetime.datetime | None]]("-- sql\nSELECT `id`, total, paid, status, meta, created_at FROM `orders`")
# rewritten: with_set = "-- sql\nSELECT `id`, tags FROM `orders`"
# rewritten: casts = SQL[tuple[int | None, str]]("-- sql\nSELECT CAST(total AS SIGNED), CAST(id AS CHAR) FROM orders")
```

`TINYINT(1)` is an `int`, not a `bool`. See the
[dialect notes](writing-queries.md#dialect-notes) for MySQL-specific types.

!!! warning "MySQL servers reject `--sql` queries"
    The marker line is part of the query: `SQL[T]` is a `str`, and PyMySQL
    and mysqlclient send it to the server unchanged. MySQL and MariaDB read
    `--` as a comment only when whitespace follows it, so `--sql\nSELECT ...`
    fails at runtime with a syntax error (1064). Write `-- sql\nSELECT ...`:
    `sqlty check` reports [`mysql-sql-marker`](../reference/rules.md#mysql-sql-marker)
    and `sqlty check --fix` changes it. Placeholders (`%s`, `%(name)s`) are
    inferred in every dialect; see [Engine](../reference/engine.md#placeholders).

## Example projects

- [`examples/02_schema_inference/`](https://github.com/jirikuncar/sqlty/tree/main/examples/02_schema_inference):
  schema passed with `--schema`.
- [`examples/03_pyproject_config/`](https://github.com/jirikuncar/sqlty/tree/main/examples/03_pyproject_config):
  schema, dialect and rules in `pyproject.toml`.
