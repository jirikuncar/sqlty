# JSON operators

The type, nullability and parameter types of PostgreSQL's `json` and `jsonb`
operators, from the rules in `rules/postgres/json.inference.toml` and
`rules/common/json.inference.toml`.

`schema.sql`:

```sql
CREATE TABLE docs (j JSON NOT NULL, b JSONB NOT NULL, t TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Accessors

`->` and `#>` keep `json` or `jsonb`; `->>` and `#>>` give text. All of them are
NULL for a missing member or path.

```sql
SELECT j -> 'a' AS ja, b -> 'a' AS ba, j #> '{a,b}' AS jp, b #> '{a,b}' AS bp, j ->> 'a' AS jt, b #>> '{a}' AS bt FROM docs
-- columns: ja: typing.Any (JSON)
-- columns: ba: typing.Any (JSONB)
-- columns: jp: typing.Any (JSON)
-- columns: bp: typing.Any (JSONB)
-- columns: jt: str | None (TEXT)
-- columns: bt: str | None (TEXT)
```

A value of unknown type is read as `jsonb`:

```sql
SELECT $1::TEXT::JSONB -> 'a' AS known, NULL -> 'a' AS unknown FROM docs
-- columns: known: typing.Any (JSONB)
-- columns: unknown: typing.Any (JSONB)
```

## Deleting

`#-` and `jsonb - key` give the `jsonb` that remains, NULL only when an operand is.

```sql
SELECT b #- '{a}' AS path, b - 'a' AS key, b - 1 AS element, b - ARRAY['a', 'b'] AS keys FROM docs
-- columns: path: typing.Any (JSONB)
-- columns: key: typing.Any (JSONB)
-- columns: element: typing.Any (JSONB)
-- columns: keys: typing.Any (JSONB)
```

## Tests

```sql
SELECT b ? 'a' AS has, b ?| ARRAY['a'] AS any_of, b ?& ARRAY['a'] AS all_of, b @> '{"a": 1}' AS contains, '{"a": 1}' <@ b AS within FROM docs
-- columns: has: bool (BOOLEAN)
-- columns: any_of: bool (BOOLEAN)
-- columns: all_of: bool (BOOLEAN)
-- columns: contains: bool (BOOLEAN)
-- columns: within: bool (BOOLEAN)
```

## Parameters

A placeholder next to a JSON value is a key (`text`), a path (`text[]`) or, for
containment, `jsonb`.

```sql
SELECT j -> $1, b ->> $2, b #> $3, j #>> $4, b #- $5 FROM docs
-- params: 1: str (TEXT)
-- params: 2: str (TEXT)
-- params: 3: list[str] (ARRAY<TEXT>)
-- params: 4: list[str] (ARRAY<TEXT>)
-- params: 5: list[str] (ARRAY<TEXT>)
```

```sql
SELECT 1 FROM docs WHERE b ? $1 AND b ?| $2 AND b ?& $3 AND b @> $4 AND $5 <@ b
-- params: 1: str (TEXT)
-- params: 2: list[str] (ARRAY<TEXT>)
-- params: 3: list[str] (ARRAY<TEXT>)
-- params: 4: typing.Any (JSONB)
-- params: 5: typing.Any (JSONB)
```

Next to a value that is not JSON, a placeholder gets no type from these
operators:

```sql
SELECT 1 FROM docs WHERE t -> $1 IS NULL
-- params: 1: None (None)
```
