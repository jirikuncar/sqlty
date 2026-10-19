# Corpus: views

A view column has the type and nullability of the expression that defines it.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Scalar subquery in a view

A scalar subquery that finds no row yields NULL, so the view column is nullable although
`tax_codes.name` is NOT NULL.

`schema.sql`:

```sql
CREATE TABLE tax_codes (code TEXT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE products (id BIGSERIAL PRIMARY KEY, ncm TEXT NOT NULL);
CREATE VIEW product_view AS
SELECT p.id AS product_id,
       (SELECT t.name FROM tax_codes t WHERE t.code = p.ncm LIMIT 1) AS description
FROM products p;
```

```sql
SELECT product_id, description FROM product_view
-- inferred: tuple[int, str | None]
```

## UNION view with one nullable branch

`first_name` is NOT NULL in one branch and nullable in the other, so the view column is
nullable.

`schema.sql`:

```sql
CREATE TABLE member (id SERIAL PRIMARY KEY, first_name TEXT NOT NULL);
CREATE TABLE test_nullability (id INTEGER PRIMARY KEY, nullable_col TEXT);
CREATE VIEW v_union_mixed AS
    SELECT id, first_name FROM member
    UNION
    SELECT id, nullable_col FROM test_nullability;
```

```sql
SELECT id, first_name FROM v_union_mixed
-- inferred: tuple[int, str | None]
```

## View over a LEFT JOIN

The view keeps the preserved side non-null and the optional side nullable.

`schema.sql`:

```sql
CREATE TABLE member (id SERIAL PRIMARY KEY);
CREATE TABLE member_team (
    id SERIAL PRIMARY KEY,
    member_id INTEGER NOT NULL REFERENCES member(id),
    team_id INTEGER NOT NULL
);
CREATE VIEW v_join AS
    SELECT m.id AS mid, mt.team_id AS tid
    FROM member m
    LEFT JOIN member_team mt ON mt.member_id = m.id;
```

```sql
SELECT mid, tid FROM v_join
-- inferred: tuple[int, int | None]
```
