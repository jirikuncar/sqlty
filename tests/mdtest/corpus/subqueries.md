# Corpus: scalar subqueries

A scalar subquery reads as NULL when it returns no row. An aggregate without `GROUP BY` or
`HAVING` always returns exactly one row; a grouped or filtered one can return none.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE member (id SERIAL PRIMARY KEY, first_name TEXT NOT NULL);
```

## count with GROUP BY or HAVING

Over an empty table the grouped query has no group and the `HAVING` rejects the only row, so
both subqueries can be NULL, while the plain `count(*)` cannot.

```sql
SELECT (SELECT count(*) FROM member) AS c,
       (SELECT count(*) FROM member GROUP BY id LIMIT 1) AS g,
       (SELECT count(*) FROM member HAVING count(*) > 0) AS h,
       (SELECT max(first_name) FROM member) AS m
-- inferred: tuple[int, int | None, int | None, str | None]
```

## EXISTS and arithmetic on an ungrouped count

`EXISTS` is true or false, and `count(*)` over no rows is 0, so `count(*) + 1` is not NULL.

```sql
SELECT EXISTS (SELECT 1 FROM member WHERE false) AS has_any,
       (SELECT count(*)::int + 1 FROM member WHERE false) AS c
-- inferred: tuple[bool, int]
```

## Correlated subqueries read every relation before them

The subqueries of one clause share their outer scope, and a join that adds a relation
gives the subqueries after it a scope that reads it too.

```sql
SELECT (SELECT a.first_name FROM member x LIMIT 1) AS from_a,
       (SELECT c.first_name FROM member x LIMIT 1) AS from_c
FROM member a
JOIN member b ON EXISTS (SELECT 1 FROM member x WHERE x.id = b.id)
JOIN member c ON EXISTS (SELECT 1 FROM member x WHERE x.id = c.id AND x.id = a.id)
WHERE EXISTS (SELECT 1 FROM member x WHERE x.id = c.id)
-- inferred: tuple[str | None, str | None]
```

```python
from sqlty import SQL

# error:
q = SQL[tuple[str | None, str | None]]("""--sql
SELECT (SELECT a.first_name FROM member x LIMIT 1) AS from_a,
       (SELECT c.first_name FROM member x LIMIT 1) AS from_c
FROM member a
JOIN member b ON EXISTS (SELECT 1 FROM member x WHERE x.id = b.id)
JOIN member c ON EXISTS (SELECT 1 FROM member x WHERE x.id = c.id AND x.id = a.id)
WHERE EXISTS (SELECT 1 FROM member x WHERE x.id = c.id)""")
```
