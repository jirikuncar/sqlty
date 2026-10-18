# Corpus: SQLite

SQLite follows the same NULL rules for joins, aggregates and set operations. An
`INTEGER PRIMARY KEY` is the rowid and never NULL; other primary keys need `NOT NULL` or
`STRICT`.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## COALESCE parameter in UPDATE ... RETURNING

The first parameter takes the column's type and may be NULL, since the column is the
fallback.

`schema.sql`:

```sql
CREATE TABLE env (id INTEGER PRIMARY KEY, create_time TEXT NOT NULL) STRICT;
```

```sql
UPDATE env SET create_time = COALESCE(?, create_time) WHERE id = ? RETURNING id, create_time
-- inferred: tuple[int, str]
-- params: 1: str (TEXT) NULL
-- params: 2: int (INT)
```

## UPDATE ... RETURNING a column set from a parameter

The parameter may be NULL, and the NOT NULL key stays non-null.

`schema.sql`:

```sql
CREATE TABLE leases (ip INTEGER NOT NULL PRIMARY KEY, client_id TEXT);
```

```sql
UPDATE leases SET client_id = ?2 WHERE ip = ?1 RETURNING ip, client_id
-- inferred: tuple[int, str | None]
```

## INSERT ... RETURNING the rowid

The rowid assigned to the new row is never NULL.

`schema.sql`:

```sql
CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT NOT NULL, description TEXT);
```

```sql
INSERT INTO projects (name, description) VALUES (?, ?) RETURNING id, name, description
-- inferred: tuple[int, str, str | None]
```

## CAST of max over a LEFT JOIN

`CAST(NULL AS REAL)` is NULL, so a group without a matching grade stays NULL.

`schema.sql`:

```sql
CREATE TABLE students (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE homework (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE grades (
    id INTEGER PRIMARY KEY,
    student_id INTEGER NOT NULL REFERENCES students(id),
    homework_id INTEGER NOT NULL REFERENCES homework(id),
    grade REAL NOT NULL
);
```

```sql
SELECT s.name, CAST(max(g.grade) AS REAL) AS grade
FROM homework h
CROSS JOIN students s
LEFT JOIN grades g ON h.id = g.homework_id AND s.id = g.student_id
WHERE h.name = ?
GROUP BY s.id
ORDER BY s.name
-- inferred: tuple[str, float | None]
-- params: 1: str (TEXT)
```

## UNION ALL with NULL padding columns

A bare `NULL` in one branch makes the column nullable and the other branch gives its type;
names come from the first branch.

`schema.sql`:

```sql
CREATE TABLE deb_packages (name TEXT NOT NULL, version TEXT NOT NULL, revision TEXT, maintainer TEXT NOT NULL);
CREATE TABLE rpm_packages (name TEXT NOT NULL, version TEXT NOT NULL, release TEXT NOT NULL, vendor TEXT NOT NULL);
```

```sql
SELECT name, version, revision, NULL AS release, maintainer AS publisher, 'deb' AS package_format
FROM deb_packages
UNION ALL
SELECT name, version, NULL, release, vendor, 'rpm'
FROM rpm_packages
-- columns: name: str (TEXT)
-- columns: version: str (TEXT)
-- columns: revision: str | None (TEXT)
-- columns: release: str | None (TEXT)
-- columns: publisher: str (TEXT)
-- columns: package_format: typing.Literal['deb', 'rpm'] (None)
```

## Correlated COUNT subquery

An ungrouped `COUNT` returns one row even when nothing matches, so the subquery is never
NULL.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY NOT NULL);
CREATE TABLE votes (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id));
```

```sql
SELECT users.id, (SELECT COUNT(votes.id) FROM votes WHERE votes.user_id = users.id) AS votes
FROM users
WHERE users.id = ?
-- inferred: tuple[int, int]
```

## SUM of a REAL product

`SUM` over REAL values is a float, and NULL over no rows.

`schema.sql`:

```sql
CREATE TABLE positions (qty REAL NOT NULL, price REAL NOT NULL);
```

```sql
SELECT SUM(qty * price) AS market_value FROM positions
-- inferred: tuple[float | None]
```

## COUNT over a grouped LEFT JOIN

The preserved side keeps its NOT NULL columns, and `COUNT` of the optional side is 0 rather
than NULL when nothing matches.

`schema.sql`:

```sql
CREATE TABLE licenses (id INTEGER PRIMARY KEY, license_key TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL);
CREATE TABLE activations (id INTEGER PRIMARY KEY, license_key TEXT NOT NULL);
```

```sql
SELECT l.id, l.created_at, COUNT(a.id) AS activation_count
FROM licenses l
LEFT JOIN activations a ON l.license_key = a.license_key
GROUP BY l.id
ORDER BY l.created_at DESC
-- inferred: tuple[int, str, int]
```

## Recursive walk over a NOT NULL primary key

Both terms select the key, declared NOT NULL because a SQLite `TEXT PRIMARY KEY` admits NULL.

`schema.sql`:

```sql
CREATE TABLE clients (id TEXT NOT NULL PRIMARY KEY, name TEXT NOT NULL, parent_id TEXT REFERENCES clients(id));
```

```sql
WITH RECURSIVE client_tree AS (
    SELECT id FROM clients WHERE id = ?
    UNION ALL
    SELECT c.id FROM clients c JOIN client_tree ct ON c.parent_id = ct.id
)
SELECT id FROM client_tree
-- inferred: tuple[str]
```
