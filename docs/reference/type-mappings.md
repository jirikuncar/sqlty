# Type Mappings

How SQL types become Python types, for casts and for schema columns alike.
Each section is executed against the implementation.

## Integers

```sql
SELECT a::INTEGER, b::INT, c::BIGINT, d::SMALLINT FROM t
-- inferred: tuple[int, int, int, int]
```

## Text

```sql
SELECT a::TEXT, b::VARCHAR, c::VARCHAR(255), d::CHAR FROM t
-- inferred: tuple[str, str, str, str]
```

## Booleans

```sql
SELECT a::BOOLEAN, b::BOOL FROM t
-- inferred: tuple[bool, bool]
```

## Floating point and decimals

```sql
SELECT a::FLOAT, b::DOUBLE PRECISION, c::REAL, d::NUMERIC, e::DECIMAL(10, 2) FROM t
-- inferred: tuple[float, float, float, float, float]
```

## Dates and times

`datetime` is imported by rewrite when needed.

```sql
SELECT a::DATE, b::TIMESTAMP, c::TIMESTAMPTZ, d::TIME, e::TIMETZ, f::INTERVAL FROM t
-- inferred: tuple[datetime.date, datetime.datetime, datetime.datetime, datetime.time, datetime.time, datetime.timedelta]
```

## UUID

```sql
SELECT a::UUID FROM t
-- inferred: tuple[uuid.UUID]
```

## JSON

```sql
SELECT a::JSON, b::JSONB FROM t
-- inferred: tuple[dict, dict]
```

## Arrays

```sql
SELECT a::INTEGER[], b::TEXT[] FROM t
-- inferred: tuple[list, list]
```

## Binary

```sql
SELECT a::BYTEA FROM t
-- inferred: tuple[bytes]
```

## Expressions

Literals, aggregates and common functions have known types; anything else is
`str`.

```sql
SELECT 1, 1.5, 'a', TRUE, COUNT(*), AVG(x::INTEGER), NOW(), LOWER(name) FROM t
-- inferred: tuple[int, float, str, bool, int, float, datetime.datetime, str]
```
