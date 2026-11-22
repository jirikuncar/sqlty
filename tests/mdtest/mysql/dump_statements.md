# MySQL statements read by their words

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## A mysqldump view

The versioned comments of a dump read as the statement they hold, without the check option.

`schema.sql`:

```sql
/*!50001 CREATE ALGORITHM=UNDEFINED */ /*!50013 DEFINER=`root`@`%` SQL SECURITY DEFINER */ /*!50001 VIEW `v` AS select 1 AS `a` */ /*!50002 WITH CASCADED CHECK OPTION */;
CREATE DEFINER = CURRENT_USER VIEW w AS SELECT 1 AS b;
/* CREATE VIEW ignored AS SELECT 1 */
```

```sql
SELECT * FROM v
-- columns: a: int (None)
```

```sql
SELECT * FROM w
-- columns: b: int (None)
```

```sql
SELECT * FROM ignored
-- columns: *: None (None)
```

## Partitioned tables

`schema.sql`:

```sql
CREATE TABLE t (id INT, PRIMARY KEY (id)) ENGINE=InnoDB PARTITION BY RANGE (id) (PARTITION p0 VALUES LESS THAN (10));
CREATE TABLE plain (id INT);
```

```sql
SELECT * FROM t
-- columns: id: int (INT)
```

```sql
SELECT * FROM plain
-- columns: id: int | None (INT)
```

## ALTER TABLE spellings

`schema.sql`:

```sql
CREATE TABLE t (x INT, a INT);
ALTER TABLE t ADD (b INT, c DECIMAL(5, 2) NOT NULL), ADD COLUMN (d INT);
ALTER TABLE t RENAME INDEX a TO b, RENAME COLUMN x TO y, RENAME KEY c TO d;
CREATE TABLE u (id INT);
ALTER TABLE u ADD e INT;
ALTER TABLE u ADD PRIMARY KEY (id);
CREATE TABLE v1 (id INT);
ALTER TABLE v1 ADD c INT, RENAME INDEX a TO;
CREATE TABLE v2 (id INT);
ALTER TABLE v2 ADD (a INT;
CREATE TABLE w (id INT);
ALTER TABLE w RENAME INDEX a TO b, RENAME INDEX c TO d;
```

```sql
SELECT * FROM t
-- columns: y: int | None (INT)
-- columns: a: int | None (INT)
-- columns: b: int | None (INT)
-- columns: c: decimal.Decimal (DECIMAL(5, 2))
-- columns: d: int | None (INT)
```

```sql
SELECT * FROM u
-- columns: id: int (INT)
-- columns: e: int | None (INT)
```

```sql
SELECT * FROM v1
-- columns: *: None (None)
```

```sql
SELECT * FROM v2
-- columns: *: None (None)
```

```sql
SELECT * FROM w
-- columns: id: int | None (INT)
```
