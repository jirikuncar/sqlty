# DuckDB nested types

The types the DuckDB Python client returns for nested values: `STRUCT` and `MAP`
are `dict`, `LIST` (`T[]`) is `list`, a fixed-size `ARRAY` (`T[3]`) is a
`tuple`, and `UNION` is the value of its active member.

```toml
[tool.sqlty]
dialect = "duckdb"
```

## STRUCT

Field names survive in `sql_type`; the Python type unites the field types.

```sql
SELECT CAST(x AS STRUCT(a INT, b VARCHAR)) AS s, CAST(x AS STRUCT(a INT, b STRUCT(c TEXT))) AS nested
-- columns: s: dict[str, int | str] (STRUCT(a INT, b VARCHAR))
-- columns: nested: dict[str, int | dict[str, str]] (STRUCT(a INT, b STRUCT(c TEXT)))
```

## MAP and LIST

```sql
SELECT CAST(x AS MAP(VARCHAR, INT)) AS m, CAST(x AS INTEGER[]) AS l, CAST(x AS INTEGER[3]) AS fixed
-- columns: m: dict[str, int] (MAP(VARCHAR, INT))
-- columns: l: list[int] (ARRAY<INT>)
-- columns: fixed: tuple[int, ...] (ARRAY<INT>[3])
```

## UNION

```sql
SELECT CAST(x AS UNION(n INT, s VARCHAR)) AS u
-- columns: u: int | str (UNION(n INT, s VARCHAR))
```

### A union inside a field

A `STRUCT`'s value type is the union of its field types; a union nested in one of them
stays inside its brackets.

```sql
SELECT CAST(x AS STRUCT(a UNION(x VARCHAR, y INT)[], b STRUCT(c UNION(x VARCHAR, y INT)))) AS s
-- columns: s: dict[str, list[str | int] | dict[str, str | int]] (STRUCT(a ARRAY<UNION(x VARCHAR, y INT)>, b STRUCT(c UNION(x VARCHAR, y INT))))
```

## Schema columns

```toml
[tool.sqlty]
dialect = "duckdb"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE events (id INTEGER NOT NULL, payload STRUCT(kind VARCHAR, n INTEGER), tags MAP(VARCHAR, VARCHAR));
```

```sql
SELECT payload, tags FROM events
-- columns: payload: dict[str, str | int] | None (STRUCT(kind VARCHAR, n INT))
-- columns: tags: dict[str, str] | None (MAP(VARCHAR, VARCHAR))
```
