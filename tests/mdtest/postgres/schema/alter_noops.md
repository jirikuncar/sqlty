# ALTER TABLE actions that do not change columns

Only actions that change a table's columns or their types can leave it with an
unknown shape. Constraints, storage options, tablespaces, partitions and the like
leave the columns as they are, also when sqlparser cannot read the statement.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Storage options

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER, name TEXT);
ALTER TABLE t SET (fillfactor = 70);
ALTER TABLE t SET TABLESPACE fast;
ALTER TABLE t ALTER COLUMN name SET STORAGE EXTERNAL;
ALTER TABLE t ALTER COLUMN name SET STATISTICS 500;
```

```sql
SELECT * FROM t
-- inferred: tuple[int, str]
```

## Constraint options sqlparser cannot read

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER, name TEXT);
ALTER TABLE t ADD CONSTRAINT u UNIQUE (name) USING INDEX TABLESPACE x;
```

```sql
SELECT * FROM t
-- inferred: tuple[int, str]
```

## A collation on a new column type

The type is the one written; the collation does not change it.

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER, name INTEGER);
ALTER TABLE t ALTER COLUMN name TYPE text COLLATE "C";
```

```sql
SELECT * FROM t
-- inferred: tuple[int, str]
```

## Partition management

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER, name TEXT) PARTITION BY RANGE (id);
ALTER TABLE t SPLIT PARTITION foo INTO (PARTITION a FOR VALUES FROM (1) TO (5));
ALTER TABLE t ATTACH PARTITION p FOR VALUES FROM (1) TO (5);
```

```sql
SELECT * FROM t
-- inferred: tuple[int, str]
```

## An action that does change columns still degrades

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER, name TEXT);
ALTER TABLE t SET SCHEMA elsewhere;
```

```sql
SELECT * FROM t
-- inferred: None
```
