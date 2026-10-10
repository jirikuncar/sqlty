# Expression Types

Beyond casts, the type of a projected expression follows its shape.
Aliases never change the type.

## Literals

```sql
SELECT 1, 1.5, 'text', TRUE, NULL
-- inferred: tuple[int, float, str, bool, str]
```

## Aliased aggregates

```sql
SELECT COUNT(*) AS total, SUM(amount) AS revenue FROM orders
-- inferred: tuple[int, float]
```

## AVG is always float

```sql
SELECT AVG(quantity::INTEGER) FROM orders
-- inferred: tuple[float]
```

## MIN and MAX keep the argument type

```sql
SELECT MIN(placed::DATE), MAX(quantity::INTEGER) FROM orders
-- inferred: tuple[datetime.date, int]
```

## COALESCE takes the first argument's type

```sql
SELECT COALESCE(nickname::TEXT, 'anonymous'), COALESCE(score::FLOAT, 0) FROM users
-- inferred: tuple[str, float]
```

## Parenthesized expressions

```sql
SELECT (id::INTEGER) FROM users
-- inferred: tuple[int]
```

## CASE takes the first branch's type

```sql
SELECT CASE WHEN active THEN 1::INTEGER ELSE 0 END FROM users
-- inferred: tuple[int]
```

## EXISTS and IN subqueries are bool

```sql
SELECT EXISTS (SELECT 1 FROM orders), id IN (SELECT user_id FROM orders) FROM users
-- inferred: tuple[bool, bool]
```

## Scalar subquery takes its column's type

```sql
SELECT (SELECT MAX(total::INTEGER) FROM orders) AS best FROM users
-- inferred: tuple[int]
```

## Window and JSON functions

```sql
SELECT ROW_NUMBER() OVER (ORDER BY id), json_build_object('id', id) FROM users
-- inferred: tuple[int, dict]
```

## Array casts are list

```sql
SELECT tags::TEXT[] FROM posts
-- inferred: tuple[list]
```

## UNION takes the leftmost SELECT

```sql
SELECT id::INTEGER FROM a UNION SELECT id FROM b UNION SELECT id FROM c
-- inferred: tuple[int]
```
