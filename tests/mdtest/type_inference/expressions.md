# Expression Types

Beyond casts, the type of a projected expression follows its shape.
Aliases never change the type.

## Literals

```sql
SELECT 1, 1.5, 'text', TRUE, NULL
-- inferred: None
```

## Aliased aggregates

```sql
SELECT COUNT(*) AS total, SUM(amount) AS revenue FROM orders
-- inferred: tuple[int, decimal.Decimal | None]
```

## AVG of integers is Decimal

```sql
SELECT AVG(quantity::INTEGER) FROM orders
-- inferred: tuple[decimal.Decimal | None]
```

## MIN and MAX keep the argument type

```sql
SELECT MIN(placed::DATE), MAX(quantity::INTEGER) FROM orders
-- inferred: tuple[datetime.date | None, int | None]
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
-- inferred: tuple[int | None]
```

## Window and JSON functions

```sql
SELECT ROW_NUMBER() OVER (ORDER BY id), json_build_object('id', id) FROM users
-- inferred: tuple[int, typing.Any]
```

## Array casts are list

```sql
SELECT tags::TEXT[] FROM posts
-- inferred: tuple[list[str]]
```

## UNION resolves each column across all branches

Branches of unknown type are skipped; numbers promote to the wider type.

```sql
SELECT id::INTEGER FROM a UNION SELECT id FROM b UNION SELECT id::BIGINT FROM c
-- inferred: tuple[int]
-- columns: id: int (BIGINT)
```
