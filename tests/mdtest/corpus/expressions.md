# Corpus: expressions

A strict operator or function is NULL when an operand is, a `CASE` is NULL when a branch can
be, and a cast never turns NULL into a value.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## CASE with a NULL branch, with and without a cast

The `CASE` takes the type of its other branch, and casting it keeps the NULL; `float` is
double precision.

`schema.sql`:

```sql
CREATE TABLE trades (id BIGSERIAL PRIMARY KEY, amount NUMERIC NOT NULL, price NUMERIC NOT NULL);
```

```sql
SELECT t.id,
       CASE WHEN t.amount = 0 THEN NULL ELSE t.price - t.amount END AS delta,
       CASE WHEN t.amount = 0 THEN NULL ELSE (t.price - t.amount)::float END AS delta_f,
       (CASE WHEN t.amount = 0 THEN NULL ELSE t.price - t.amount END)::float AS delta_cast
FROM trades t
-- inferred: tuple[int, decimal.Decimal | None, float | None, float | None]
```

## Square root of a nullable column

`|/` is strict: NULL in, NULL out.

`schema.sql`:

```sql
CREATE TABLE test_sqrt (n DOUBLE PRECISION, m DOUBLE PRECISION NOT NULL);
```

```sql
SELECT |/ n AS root_n, |/ m AS root_m FROM test_sqrt
-- inferred: tuple[float | None, float]
```

## User-defined function

<!-- mdtest:xfail a user-defined function result is typed non-null, str instead of str | None -->

A PL/pgSQL function can `RETURN NULL` for any declared return type, so its result is
nullable.

`schema.sql`:

```sql
CREATE FUNCTION mark_email_verified(p_user_id UUID)
RETURNS TEXT AS $$
BEGIN
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
```

```sql
SELECT mark_email_verified($1) AS result
-- inferred: tuple[str | None]
-- params: 1: uuid.UUID (UUID)
```

## SERIAL in arithmetic

`SERIAL` is `integer` with a default, so it adds like one.

`schema.sql`:

```sql
CREATE TABLE counters (id SERIAL PRIMARY KEY, step INTEGER NOT NULL);
```

```sql
SELECT id + step AS next_id FROM counters
-- inferred: tuple[int]
```

## Column name of a CASE

An unaliased `CASE` takes the name of its `ELSE` branch when that is a column, as
PostgreSQL names it, and `case` otherwise.

`schema.sql`:

```sql
CREATE TABLE counters (id INTEGER PRIMARY KEY, step INTEGER);
```

```sql
SELECT CASE WHEN id > 0 THEN id ELSE step END FROM counters
-- columns: step: int | None (INT)
```

```sql
SELECT CASE WHEN id > 0 THEN step ELSE 0 END FROM counters
-- columns: case: int | None (INT)
```
