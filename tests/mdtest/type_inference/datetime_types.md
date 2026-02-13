# Datetime Type Inference

## TIMESTAMP infers datetime.datetime

```sql
SELECT created_at::TIMESTAMP FROM events
```

```python
# inferred: tuple[datetime.datetime]
```

## DATE infers datetime.date

```sql
SELECT birth_date::DATE FROM users
```

```python
# inferred: tuple[datetime.date]
```

## TIME infers datetime.time

```sql
SELECT start_time::TIME FROM schedule
```

```python
# inferred: tuple[datetime.time]
```

## INTERVAL infers datetime.timedelta

```sql
SELECT duration::INTERVAL FROM tasks
```

```python
# inferred: tuple[datetime.timedelta]
```

## Multiple datetime types

```sql
SELECT created_at::TIMESTAMP, birth_date::DATE, start_time::TIME FROM users
```

```python
# inferred: tuple[datetime.datetime, datetime.date, datetime.time]
```
