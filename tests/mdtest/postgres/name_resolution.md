# PostgreSQL name resolution

PostgreSQL resolves a bare name through the `search_path`, folds an unquoted
identifier to lower case, keeps a quoted one exactly, gives every table a few
system columns and lets `pg_catalog.` qualify a built-in function. MySQL and SQLite
keep their own rules (see the last section).

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
unknown-function = "error"
```

## A bare name resolves only through the search_path

`schema.sql`:

```sql
CREATE TABLE a.t (x INTEGER NOT NULL);
CREATE TABLE public.u (y TEXT NOT NULL);
```

```python
from sqlty import SQL

# error: [unknown-table] "table `t` is not defined in the schema"
a = SQL[tuple[int]]("--sql\nSELECT x FROM t")
b = SQL[tuple[int]]("--sql\nSELECT x FROM a.t")
c = SQL[tuple[str]]("--sql\nSELECT y FROM u")
d = SQL[tuple[str]]("--sql\nSELECT y FROM public.u")
```

### A schema on the search_path

`schema.sql`:

```sql
SET search_path TO a, public;
CREATE TABLE a.t (x INTEGER NOT NULL);
CREATE TABLE public.u (y TEXT NOT NULL);
```

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT x FROM t")
b = SQL[tuple[str]]("--sql\nSELECT y FROM u")
```

### A table of a schema off the path

`schema.sql`:

```sql
SET search_path TO a;
CREATE TABLE a.t (x INTEGER NOT NULL);
CREATE TABLE b.t (x TEXT NOT NULL);
CREATE TABLE b.only_in_b (z INTEGER NOT NULL);
```

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT x FROM t")
b = SQL[tuple[str]]("--sql\nSELECT x FROM b.t")
# error: [unknown-table] "table `only_in_b` is not defined in the schema"
c = SQL[tuple[int]]("--sql\nSELECT z FROM only_in_b")
d = SQL[tuple[int]]("--sql\nSELECT z FROM b.only_in_b")
```

## Quoted identifiers keep their case

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER NOT NULL, "userId" INTEGER NOT NULL, "Name" TEXT NOT NULL);
CREATE TABLE "Users" (id INTEGER NOT NULL);
CREATE TABLE users (id TEXT NOT NULL);
```

```python
from sqlty import SQL

# error: [unknown-column] "column `ID` does not exist in `t`"
a = SQL[tuple[int]]('--sql\nSELECT "ID" FROM t')
b = SQL[tuple[int]]("--sql\nSELECT ID FROM t")
# error: [unknown-column] "column `userId` does not exist in `t`"
c = SQL[tuple[int]]("--sql\nSELECT userId FROM t")
d = SQL[tuple[int]]('--sql\nSELECT "userId" FROM t')
# error: [unknown-column] "column `name` does not exist in `t`"
e = SQL[tuple[str]]("--sql\nSELECT name FROM t")
f = SQL[tuple[str]]('--sql\nSELECT "Name" FROM t')
g = SQL[tuple[int]]('--sql\nSELECT t."userId" FROM t')
```

### Quoted and unquoted relations are different

```python
from sqlty import SQL

a = SQL[tuple[int]]('--sql\nSELECT id FROM "Users"')
b = SQL[tuple[str]]("--sql\nSELECT id FROM users")
c = SQL[tuple[str]]("--sql\nSELECT id FROM USERS")
# error: [unknown-table] "table `USERS` is not defined in the schema"
d = SQL[tuple[str]]('--sql\nSELECT id FROM "USERS"')
```

### Aliases and CTEs

```python
from sqlty import SQL

a = SQL[tuple[int]]('--sql\nSELECT s."userId" FROM t AS s')
b = SQL[tuple[int]]('--sql\nSELECT "userId" FROM (SELECT "userId" FROM t) AS s')
c = SQL[tuple[int]]('--sql\nWITH "Q" AS (SELECT id FROM t) SELECT id FROM "Q"')
d = SQL[tuple[int]]("--sql\nSELECT fooBar FROM (SELECT id AS FooBar FROM t) AS s")
```

### Unquoted aliases are lower case, quoted ones are exact

```sql
SELECT id AS FooBar, id AS "FooBar" FROM t
-- columns: foobar: int (INT)
-- columns: FooBar: int (INT)
```

## System columns

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER NOT NULL);
CREATE TABLE legacy (oid INTEGER NOT NULL, id INTEGER NOT NULL);
```

`oid` is not a column of a user table since PostgreSQL 12, unless the table declares it.

```python
from sqlty import SQL

# error: [unknown-column] "column `oid` does not exist in `t`"
a = SQL[tuple[int]]("--sql\nSELECT oid FROM t")
b = SQL[tuple[int]]("--sql\nSELECT oid FROM legacy")
```

```sql
SELECT ctid, xmin, xmax, cmin, cmax, tableoid FROM t
-- columns: ctid: str (TID)
-- columns: xmin: str (XID)
-- columns: xmax: str (XID)
-- columns: cmin: str (CID)
-- columns: cmax: str (CID)
-- columns: tableoid: int (OID)
```

A column the table declares wins over a system column of the same name.

```python
from sqlty import SQL

a = SQL[tuple[str, int]]("--sql\nSELECT t.ctid, t.id FROM t")
```

## Schema-qualified built-in functions

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER NOT NULL);
```

```python
from sqlty import SQL

a = SQL[tuple[str]]("--sql\nSELECT pg_catalog.lower('A') AS v")
b = SQL[tuple[str]]("--sql\nSELECT lower('A') AS v")
c = SQL[tuple[int]]("--sql\nSELECT pg_catalog.length('abc') AS v")
# error: [unknown-function] "function `other.lower` is not known to sqlty, so its result is untyped"
d = SQL[tuple[str]]("--sql\nSELECT other.lower('A') AS v")
# error: [unknown-function] "function `"LOWER"` is not known to sqlty, so its result is untyped"
e = SQL[tuple[str]]('--sql\nSELECT "LOWER"(id::text) AS v FROM t')
```

## MySQL and SQLite are unchanged

Names compare case-insensitively, a schema qualifier is dropped, and there is no
`search_path`.

### mysql

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
```

`schema.sql`:

```sql
CREATE TABLE Users (Id INT NOT NULL, `userId` INT NOT NULL);
```

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT id FROM users")
b = SQL[tuple[int]]("--sql\nSELECT ID FROM db.USERS")
c = SQL[tuple[int]]("--sql\nSELECT `USERID` FROM users")
d = SQL[tuple[int]]("--sql\nSELECT userid FROM `Users`")
```

### sqlite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
```

`schema.sql`:

```sql
CREATE TABLE Users (Id INTEGER NOT NULL, "userId" INTEGER NOT NULL);
```

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT id FROM users")
b = SQL[tuple[int]]('--sql\nSELECT "USERID" FROM "USERS"')
c = SQL[tuple[int]]("--sql\nSELECT USERID FROM main.users")
d = SQL[tuple[int]]("--sql\nSELECT rowid FROM users")
e = SQL[tuple[int]]("--sql\nSELECT oid FROM users")
```
