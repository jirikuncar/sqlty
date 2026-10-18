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
