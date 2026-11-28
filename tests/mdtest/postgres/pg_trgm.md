# pg_trgm

Trigram similarity, from `rules/postgres/extensions/pg_trgm.inference.toml`, once the
schema runs `CREATE EXTENSION pg_trgm`.

`schema.sql`:

```sql
CREATE EXTENSION pg_trgm;
CREATE TABLE users (id int PRIMARY KEY, name text NOT NULL, email text);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Functions

```sql
SELECT similarity(name, 'jo') AS s, word_similarity('jo', name) AS w, strict_word_similarity('jo', email) AS sw, show_trgm(name) AS trigrams, show_limit() AS threshold, similarity_op(name, 'jo') AS similar FROM users
-- columns: s: float (FLOAT)
-- columns: w: float (FLOAT)
-- columns: sw: float | None (FLOAT)
-- columns: trigrams: list[str] (ARRAY<TEXT>)
-- columns: threshold: float (FLOAT)
-- columns: similar: bool (BOOLEAN)
-- rules: s: postgres.pg_trgm.similarity / postgres.pg_trgm.similarity
-- rules: w: postgres.pg_trgm.similarity / postgres.pg_trgm.similarity
-- rules: sw: postgres.pg_trgm.similarity / postgres.pg_trgm.similarity
-- rules: trigrams: postgres.pg_trgm.trigrams / postgres.pg_trgm.trigrams
-- rules: threshold: postgres.pg_trgm.similarity / postgres.pg_trgm.similarity
-- rules: similar: postgres.pg_trgm.tests / postgres.pg_trgm.tests
```

## Operators

`%` tests similarity and `<->` gives the distance of two strings.

```sql
SELECT name % 'jo' AS similar, name <-> 'jo' AS distance, email <-> name AS maybe FROM users ORDER BY name <-> 'jo'
-- columns: similar: bool (BOOLEAN)
-- columns: distance: float (FLOAT)
-- columns: maybe: float | None (FLOAT)
-- rules: similar: postgres.pg_trgm.similar / postgres.pg_trgm.similar
-- rules: distance: postgres.pg_trgm.distance / postgres.pg_trgm.distance
-- rules: maybe: postgres.pg_trgm.distance / postgres.pg_trgm.distance
```

```sql
SELECT id FROM users WHERE name % $1 AND $2 <-> name < 0.5
-- params: 1: str (TEXT)
-- params: 2: str (TEXT)
```

## Without `CREATE EXTENSION`

`schema.sql`:

```sql
CREATE TABLE users (id int PRIMARY KEY, name text NOT NULL);
```

```sql
SELECT similarity(name, 'jo') AS s FROM users
-- columns: s: None (None)
```

```python
from sqlty import SQL

# error: [unknown-function] "function `similarity` is not known to sqlty, so its result is untyped; it comes with `CREATE EXTENSION pg_trgm`, which the schema lacks"
q = SQL[tuple[float]]("--sql\nSELECT similarity(name, 'jo') FROM users")
```
