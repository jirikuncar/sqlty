# Types with `driver = "asyncpg"`

asyncpg loads `json` and `jsonb` as their text, bit strings, geometric types and ranges
as its own classes, and a multirange as a list of ranges.

```toml
[tool.sqlty]
schema = "schema.sql"
driver = "asyncpg"
```

`schema.sql`:

```sql
CREATE TABLE shapes (
    id integer PRIMARY KEY,
    doc jsonb NOT NULL,
    meta json,
    flags bit(3),
    mask varbit,
    at point,
    edge lseg,
    area box,
    ring circle,
    during tstzrange,
    spans int4multirange,
    tags jsonb[]
);
```

## Columns

```sql
SELECT doc, meta, flags, mask, at, edge, area, ring, during, spans FROM shapes
-- columns: doc: str (JSONB)
-- columns: meta: str | None (JSON)
-- columns: flags: asyncpg.BitString | None (BIT(3))
-- columns: mask: asyncpg.BitString | None (VARBIT)
-- columns: at: asyncpg.Point | None (POINT)
-- columns: edge: asyncpg.LineSegment | None (LSEG)
-- columns: area: asyncpg.Box | None (BOX)
-- columns: ring: asyncpg.Circle | None (CIRCLE)
-- columns: during: asyncpg.Range[datetime.datetime] | None (TSTZRANGE)
-- columns: spans: list[asyncpg.Range[int]] | None (INT4MULTIRANGE)
```

## JSON expressions are text too

The value of a JSON operator or function is `json` or `jsonb`, which asyncpg loads as
text whatever its shape.

```sql
SELECT doc -> 'a' AS a, jsonb_build_object('id', id) AS obj, json_agg(id) AS ids, doc ->> 'b' AS b FROM shapes GROUP BY doc
-- columns: a: str | None (JSONB)
-- columns: obj: str (JSONB)
-- columns: ids: str (JSON)
-- columns: b: str | None (TEXT)
```

## An array of JSON is a list of text

```sql
SELECT tags FROM shapes
-- columns: tags: list[str] | None (ARRAY<JSONB>)
```

## A column typed by name keeps its type

```toml
[tool.sqlty]
schema = "schema.sql"
driver = "asyncpg"

[tool.sqlty.types]
"shapes.doc" = "app.Doc"
```

```sql
SELECT doc, meta FROM shapes
-- columns: doc: app.Doc (JSONB)
-- columns: meta: str | None (JSON)
```

## Branches of a set operation agree

```sql
SELECT doc -> 'a' AS a, at FROM shapes UNION ALL SELECT doc, point(1, 2) FROM shapes
-- columns: a: str | None (JSONB)
-- columns: at: asyncpg.Point | None (POINT)
```

## An array expression is a list of what asyncpg loads

```sql
SELECT array_agg(flags) AS f, array_agg(at) AS p, array_agg(doc) AS d FROM shapes
-- columns: f: list[asyncpg.BitString] | None (ARRAY<BIT(3)>)
-- columns: p: list[asyncpg.Point] | None (ARRAY<POINT>)
-- columns: d: list[str] | None (ARRAY<JSONB>)
```

## A column typed by name as the default keeps it

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.types]
NUMERIC = "float"
"prices.amount" = "decimal.Decimal"
```

`schema.sql`:

```sql
CREATE TABLE prices (amount numeric NOT NULL, history numeric[] NOT NULL);
```

```sql
SELECT amount, amount + 1 AS next, history, array_agg(amount) AS amounts FROM prices GROUP BY amount, history
-- columns: amount: decimal.Decimal (DECIMAL)
-- columns: next: float (DECIMAL)
-- columns: history: list[float] (ARRAY<DECIMAL>)
-- columns: amounts: list[float] (ARRAY<DECIMAL>)
```
