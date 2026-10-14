# PARTITION OF, INHERITS and LIKE

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## PARTITION OF

A partition has the columns of its parent, including the ones added later.

`schema.sql`:

```sql
CREATE TABLE measurements (at TIMESTAMPTZ NOT NULL, value DOUBLE PRECISION) PARTITION BY RANGE (at);
CREATE TABLE measurements_2024 PARTITION OF measurements
    FOR VALUES FROM ('2024-01-01') TO ('2025-01-01');
CREATE TABLE measurements_rest PARTITION OF measurements DEFAULT;
ALTER TABLE measurements ADD COLUMN sensor TEXT;
```

```sql
SELECT * FROM measurements_2024
-- inferred: tuple[datetime.datetime, float, str]
```

```sql
SELECT sensor FROM measurements_rest
-- inferred: tuple[str]
```

## INHERITS

The child lists the parent's columns first, then its own.

`schema.sql`:

```sql
CREATE TABLE cities (name TEXT, population INTEGER);
CREATE TABLE capitals (state CHAR(2)) INHERITS (cities);
ALTER TABLE cities ADD COLUMN elevation INTEGER;
```

```sql
SELECT * FROM capitals
-- inferred: tuple[str, int, str, int]
```

## ALTER TABLE ONLY does not reach children

`schema.sql`:

```sql
CREATE TABLE cities (name TEXT);
CREATE TABLE capitals (state TEXT) INHERITS (cities);
ALTER TABLE ONLY cities ADD COLUMN elevation INTEGER;
```

```sql
SELECT * FROM capitals
-- inferred: tuple[str, str]
```

## LIKE

`LIKE` copies the columns at that point; later changes to the source do not follow.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE users_archive (LIKE users INCLUDING ALL, archived_at TIMESTAMPTZ);
ALTER TABLE users ADD COLUMN email TEXT;
```

```sql
SELECT * FROM users_archive
-- inferred: tuple[int, str, datetime.datetime]
```

## The parent may come later

`schema.sql`:

```sql
CREATE TABLE capitals (state TEXT) INHERITS (cities);
CREATE TABLE copy (LIKE cities);
CREATE TABLE cities (name TEXT, population INTEGER);
```

```sql
SELECT * FROM capitals
-- inferred: tuple[str, int, str]
```

```sql
SELECT * FROM copy
-- inferred: tuple[str, int]
```

## A parent the schema never defines

The table exists, but its columns are not known, so none is typed.

`schema.sql`:

```sql
CREATE TABLE capitals (state TEXT) INHERITS (cities);
CREATE TABLE copy (LIKE cities);
```

```sql
SELECT state FROM capitals
-- inferred: None
```

```sql
SELECT * FROM copy
-- inferred: None
```
