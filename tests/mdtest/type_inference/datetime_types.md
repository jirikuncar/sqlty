# Datetime Type Inference

## TIMESTAMP infers datetime.datetime

```sql
SELECT created_at::TIMESTAMP FROM events
-- inferred: tuple[datetime.datetime]
```

## DATE infers datetime.date

```sql
SELECT birth_date::DATE FROM users
-- inferred: tuple[datetime.date]
```

## TIME infers datetime.time

```sql
SELECT start_time::TIME FROM schedule
-- inferred: tuple[datetime.time]
```

## INTERVAL infers datetime.timedelta

```sql
SELECT duration::INTERVAL FROM tasks
-- inferred: tuple[datetime.timedelta]
```

## Multiple datetime types

```sql
SELECT created_at::TIMESTAMP, birth_date::DATE, start_time::TIME FROM users
-- inferred: tuple[datetime.datetime, datetime.date, datetime.time]
```

## Typed string literals

A literal prefixed with its type, like `DATE '2024-01-01'`, has that type.

```sql
SELECT DATE '2024-01-01' AS d, TIME '12:00' AS t, TIMESTAMP '2024-01-01 00:00:00' AS ts,
       TIMESTAMPTZ '2024-01-01 00:00:00+00' AS tstz, INTERVAL '1 day' AS i
-- columns: d: datetime.date (DATE)
-- columns: t: datetime.time (TIME)
-- columns: ts: datetime.datetime (TIMESTAMP)
-- columns: tstz: datetime.datetime (TIMESTAMPTZ)
-- columns: i: datetime.timedelta (INTERVAL)
```
