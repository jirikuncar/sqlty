# Function Result Types

## SUM function without explicit cast

```sql
SELECT SUM(amount) FROM orders
-- inferred: tuple[float]
```

## AVG function without explicit cast

```sql
SELECT AVG(score) FROM tests
-- inferred: tuple[float]
```

## TRIM function is recognized

```sql
SELECT TRIM(' ' FROM name) FROM users
-- inferred: tuple[str]
```

## CONCAT function

```sql
SELECT CONCAT(first_name, ' ', last_name) FROM users
-- inferred: tuple[str]
```

## Unknown function inference returns None (1)

```sql
SELECT MY_CUSTOM_FUNCTION(data) FROM table_name
-- inferred: tuple[str]
```

## Unknown function inference returns None (2)

```sql
SELECT WEIRD_FUNC(x, y) FROM z
-- inferred: tuple[str]
```

## Unknown function inference returns None (3)

```sql
SELECT EXTRACT(YEAR FROM created_at) FROM events
-- inferred: tuple[str]
```

## NOW() function to hit line 115

```sql
SELECT NOW()
-- inferred: tuple[datetime.datetime]
```

## CURRENT_TIMESTAMP to hit line 115

```sql
SELECT CURRENT_TIMESTAMP
-- inferred: tuple[datetime.datetime]
```

## SQL that results in empty column list

```sql
SELECT
-- inferred: None
```
