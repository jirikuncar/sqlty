# intarray

Integer arrays as sets, from `rules/postgres/extensions/intarray.inference.toml`, once the
schema runs `CREATE EXTENSION intarray`.

`schema.sql`:

```sql
CREATE EXTENSION intarray;
CREATE TABLE posts (id int PRIMARY KEY, tags int[] NOT NULL, extra int[]);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Functions

```sql
SELECT icount(tags) AS n, idx(tags, 3) AS at, sort(tags) AS sorted, sort_desc(extra) AS down, uniq(sort(tags)) AS distinct_tags, subarray(tags, 1, 2) AS part, intset(id) AS one FROM posts
-- columns: n: int (INT)
-- columns: at: int (INT)
-- columns: sorted: list[int] (ARRAY<INT>)
-- columns: down: list[int] | None (ARRAY<INT>)
-- columns: distinct_tags: list[int] (ARRAY<INT>)
-- columns: part: list[int] (ARRAY<INT>)
-- columns: one: list[int] (ARRAY<INT>)
-- rules: n: postgres.intarray.count / postgres.intarray.count
-- rules: at: postgres.intarray.count / postgres.intarray.count
-- rules: sorted: postgres.intarray.array / postgres.intarray.array
-- rules: down: postgres.intarray.array / postgres.intarray.array
-- rules: distinct_tags: postgres.intarray.array / postgres.intarray.array
-- rules: part: postgres.intarray.array / postgres.intarray.array
-- rules: one: postgres.intarray.array / postgres.intarray.array
```

## Operators

```sql
SELECT tags & extra AS common, tags | 5 AS with_five, tags + extra AS joined, tags - 5 AS without_five, tags # 5 AS position FROM posts
-- columns: common: list[int] | None (ARRAY<INT>)
-- columns: with_five: list[int] (ARRAY<INT>)
-- columns: joined: list[int] | None (ARRAY<INT>)
-- columns: without_five: list[int] (ARRAY<INT>)
-- columns: position: int (INT)
-- rules: common: postgres.intarray.combine / postgres.intarray.combine
-- rules: with_five: postgres.intarray.combine / postgres.intarray.combine
-- rules: joined: postgres.intarray.combine / postgres.intarray.combine
-- rules: without_five: postgres.intarray.combine / postgres.intarray.combine
-- rules: position: postgres.intarray.index / postgres.intarray.index
```

## Without `CREATE EXTENSION`

`schema.sql`:

```sql
CREATE TABLE posts (id int PRIMARY KEY, tags int[] NOT NULL);
```

```sql
SELECT icount(tags) AS n, tags & tags AS common FROM posts
-- columns: n: None (None)
-- columns: common: None (None)
```

```python
from sqlty import SQL

# error: [unknown-function] "function `icount` is not known to sqlty, so its result is untyped; it comes with `CREATE EXTENSION intarray`, which the schema lacks"
q = SQL[tuple[int]]("--sql\nSELECT icount(tags) FROM posts")
```
