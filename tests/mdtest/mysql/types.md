# MySQL Types

## Column types from the schema

`schema.sql`:
```sql
CREATE TABLE items (
    a TINYINT,
    b MEDIUMINT,
    c INT3,
    d MIDDLEINT,
    e INT8,
    f INT(11) SIGNED,
    g INT UNSIGNED,
    h BIGINT(20) UNSIGNED,
    i FLOAT UNSIGNED,
    j DOUBLE,
    k DECIMAL(10,2) UNSIGNED,
    l BIT(8),
    m YEAR,
    n DATETIME(6),
    o TIMESTAMP,
    p ENUM('x', 'y'),
    q SET('x', 'y'),
    r JSON,
    s MEDIUMBLOB,
    t LONGTEXT,
    u POINT
);
```

```toml
[sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT * FROM items
```

```python
# inferred: tuple[int, int, int, int, int, int, int, int, float, float, float, str, int, datetime.datetime, datetime.datetime, str, str, dict, str, str, str]
# expanded: SELECT a::TINYINT, b::MEDIUMINT, c::MEDIUMINT, d::MEDIUMINT, e::BIGINT, f::INT(11), g::UINT, h::UBIGINT(20), i::UDOUBLE, j::DOUBLE, k::UDECIMAL(10, 2), l::BIT(8), m::YEAR, n::DATETIME(6), o::TIMESTAMPTZ, p::ENUM('X', 'Y'), q::SET('X', 'Y'), r::JSON, s::BLOB, t::TEXT, u::POINT FROM items
```

## Integer CASTs

```toml
[sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(a AS SIGNED), CAST(b AS UNSIGNED INTEGER), CAST(c AS YEAR) FROM t
```

```python
# inferred: tuple[int, int, int]
```

## Multi-valued index CAST to an array

```toml
[sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(doc->'$.zips' AS UNSIGNED ARRAY) FROM t
```

```python
# inferred: tuple[list]
```

## Bracketed array types are not MySQL

```toml
[sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(tags AS INT[]) FROM t
```

```python
# inferred: None
```
