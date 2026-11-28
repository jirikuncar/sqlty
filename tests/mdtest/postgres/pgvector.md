# pgvector

Vectors and their distances, from `rules/postgres/extensions/vector.inference.toml`, once the
schema runs `CREATE EXTENSION vector`. psycopg returns `vector`, `halfvec` and `sparsevec`
values as text; with `pgvector.psycopg.register_vector`, set
`[tool.sqlty.types] VECTOR = "pgvector.Vector"`.

`schema.sql`:

```sql
CREATE EXTENSION vector;
CREATE TABLE items (id int PRIMARY KEY, embedding vector(3) NOT NULL, draft vector(3));
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Columns

```sql
SELECT embedding, draft FROM items
-- columns: embedding: str (VECTOR(3))
-- columns: draft: str | None (VECTOR(3))
```

## Operators

`<->` and `<=>` give a distance; `+`, `-` and `*` work element by element.

```sql
SELECT embedding <-> draft AS l2, embedding <=> '[1,2,3]'::vector AS cosine, embedding + embedding AS sum, embedding - draft AS difference, embedding * embedding AS product FROM items
-- columns: l2: float | None (DOUBLE)
-- columns: cosine: float (DOUBLE)
-- columns: sum: str (VECTOR(3))
-- columns: difference: str | None (VECTOR(3))
-- columns: product: str (VECTOR(3))
-- rules: l2: postgres.vector.distance / postgres.vector.distance
-- rules: cosine: postgres.vector.distance / postgres.vector.distance
-- rules: sum: postgres.vector.arithmetic / postgres.vector.arithmetic
-- rules: difference: postgres.vector.arithmetic / postgres.vector.arithmetic
-- rules: product: postgres.vector.arithmetic / postgres.vector.arithmetic
```

```sql
SELECT id, embedding <=> $1 AS distance FROM items
-- params: 1: str (VECTOR(3))
```

## Functions

```sql
SELECT l2_distance(embedding, draft) AS l2, cosine_distance(embedding, embedding) AS cosine, inner_product(embedding, embedding) AS inner, l1_distance(embedding, embedding) AS taxicab, vector_norm(embedding) AS norm, vector_dims(embedding) AS dims FROM items
-- columns: l2: float | None (DOUBLE)
-- columns: cosine: float (DOUBLE)
-- columns: inner: float (DOUBLE)
-- columns: taxicab: float (DOUBLE)
-- columns: norm: float (DOUBLE)
-- columns: dims: int (INT)
-- rules: l2: postgres.vector.distances / postgres.vector.distances
-- rules: cosine: postgres.vector.distances / postgres.vector.distances
-- rules: inner: postgres.vector.distances / postgres.vector.distances
-- rules: taxicab: postgres.vector.distances / postgres.vector.distances
-- rules: norm: postgres.vector.distances / postgres.vector.distances
-- rules: dims: postgres.vector.dimensions / postgres.vector.dimensions
```

pgvector 0.7 adds `halfvec`, `sparsevec`, `||` and the functions below:

```sql
SELECT l2_normalize(embedding) AS unit, subvector(embedding, 1, 2) AS head, binary_quantize(embedding) AS bits, subvector(embedding::halfvec(3), 1, 2) AS half, embedding::halfvec(3) || embedding::halfvec(3) AS halves, embedding::halfvec(3) <-> embedding::halfvec(3) AS half_l2, '{1:1}/3'::sparsevec <=> '{2:1}/3'::sparsevec AS sparse_cosine, embedding || draft AS joined FROM items
-- columns: unit: str (VECTOR(3))
-- columns: head: str (VECTOR)
-- columns: bits: str (BIT)
-- columns: half: str (HALFVEC)
-- columns: halves: str (HALFVEC)
-- columns: half_l2: float (DOUBLE)
-- columns: sparse_cosine: float (DOUBLE)
-- columns: joined: str | None (VECTOR)
-- rules: unit: postgres.vector.normalized / postgres.vector.normalized
-- rules: head: postgres.vector.subvector / postgres.vector.subvector
-- rules: bits: postgres.vector.quantize / postgres.vector.quantize
-- rules: half: postgres.vector.subvector-halfvec / postgres.vector.subvector-halfvec
-- rules: halves: postgres.vector.concat-halfvec / postgres.vector.concat-halfvec
-- rules: half_l2: postgres.vector.distance-halfvec / postgres.vector.distance-halfvec
-- rules: sparse_cosine: postgres.vector.distance-sparsevec / postgres.vector.distance-sparsevec
-- rules: joined: postgres.vector.concat / postgres.vector.concat
```

## With `register_vector`

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.types]
VECTOR = "pgvector.Vector"
```

```python
q = "--sql\nSELECT embedding, l2_normalize(embedding) FROM items"
# rewritten: import pgvector
# rewritten: q = SQL[tuple[pgvector.Vector, pgvector.Vector]]("--sql\nSELECT embedding, l2_normalize(embedding) FROM items")
```

## Without `CREATE EXTENSION`

`schema.sql`:

```sql
CREATE TABLE items (id int PRIMARY KEY, label text NOT NULL);
```

```sql
SELECT l2_distance(label, label) AS l2 FROM items
-- columns: l2: None (None)
```

```python
from sqlty import SQL

# error: [unknown-function] "function `l2_distance` is not known to sqlty, so its result is untyped; it comes with `CREATE EXTENSION vector`, which the schema lacks"
q = SQL[tuple[float]]("--sql\nSELECT l2_distance(label, label) FROM items")
```
