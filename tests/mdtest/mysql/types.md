# MySQL Types

## Integer columns

`INT3` and `MIDDLEINT` are aliases for `MEDIUMINT`, and `INT8` for `BIGINT`. A
trailing `SIGNED` is the default and is dropped.

`schema.sql`:
```sql
CREATE TABLE items (
    a TINYINT,
    b MEDIUMINT,
    c INT3,
    d MIDDLEINT,
    e INT8,
    f INT(11) SIGNED,
    g YEAR
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
-- inferred: tuple[int, int, int, int, int, int, int]
-- expanded: SELECT a::TINYINT, b::MEDIUMINT, c::MEDIUMINT, d::MEDIUMINT, e::BIGINT, f::INT(11), g::YEAR FROM items
```

## Unsigned columns

`UNSIGNED` columns are spelled with a `U` prefix (`INT UNSIGNED` is `UINT`), and
`FLOAT UNSIGNED` becomes `UDOUBLE`. They map to the same Python types as their
signed forms.

`schema.sql`:
```sql
CREATE TABLE items (
    a INT UNSIGNED,
    b BIGINT(20) UNSIGNED,
    c FLOAT UNSIGNED,
    d DECIMAL(10,2) UNSIGNED
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
-- inferred: tuple[int, int, float, float]
-- expanded: SELECT a::UINT, b::UBIGINT(20), c::UDOUBLE, d::UDECIMAL(10, 2) FROM items
```

## Temporal and other columns

MySQL `TIMESTAMP` is stored in UTC, so it reads as `TIMESTAMPTZ`. Blob and text
variants collapse to `BLOB` and `TEXT`. `ENUM` and `SET` values are upper-cased
in the expansion, as sqlglot spells them; the column type is still `str`.
Unmapped types such as `BIT` and spatial `POINT` are `str`.

`schema.sql`:
```sql
CREATE TABLE items (
    a DOUBLE,
    b BIT(8),
    c DATETIME(6),
    d TIMESTAMP,
    e ENUM('x', 'y'),
    f SET('x', 'y'),
    g JSON,
    h MEDIUMBLOB,
    i LONGTEXT,
    j POINT
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
-- inferred: tuple[float, str, datetime.datetime, datetime.datetime, str, str, dict, bytes, str, str]
-- expanded: SELECT a::DOUBLE, b::BIT(8), c::DATETIME(6), d::TIMESTAMPTZ, e::ENUM('X', 'Y'), f::SET('X', 'Y'), g::JSON, h::BLOB, i::TEXT, j::POINT FROM items
```

## Unsigned NUMERIC and FIXED are rejected

`UNSIGNED` applies to `DECIMAL` (or `DEC`), not to its synonyms `NUMERIC` and
`FIXED`, so nothing is inferred.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(a AS NUMERIC(8,3) UNSIGNED), CAST(b AS FIXED UNSIGNED) FROM t
-- inferred: None
```

## Integer CASTs

`SIGNED` and `UNSIGNED` (with or without `INTEGER`) are MySQL's integer cast
targets.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(a AS SIGNED), CAST(b AS UNSIGNED INTEGER), CAST(c AS YEAR) FROM t
-- inferred: tuple[int, int, int]
```

## Multi-valued index CAST to an array

MySQL's only array type is the `... ARRAY` cast used by multi-valued indexes.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(doc->'$.zips' AS UNSIGNED ARRAY) FROM t
-- inferred: tuple[list]
```

## Bracketed array types are not MySQL

PostgreSQL's `INT[]` is a parse error in MySQL, so nothing is inferred.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(tags AS INT[]) FROM t
-- inferred: None
```

## INT3, MIDDLEINT and ZEROFILL

<!-- mdtest:backend rs -->

`INT3 UNSIGNED` and `MIDDLEINT UNSIGNED` are `UMEDIUMINT`. `ZEROFILL` implies
`UNSIGNED`.

`schema.sql`:
```sql
CREATE TABLE items (
    a INT3 UNSIGNED,
    b MIDDLEINT(8) UNSIGNED,
    c INT(5) ZEROFILL,
    d BIGINT UNSIGNED ZEROFILL
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
-- inferred: tuple[int, int, int, int]
-- expanded: SELECT a::UMEDIUMINT, b::UMEDIUMINT(8), c::UINT(5), d::UBIGINT FROM items
```
