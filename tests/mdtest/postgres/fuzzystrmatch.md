# fuzzystrmatch

Phonetic codes and edit distances, from
`rules/postgres/extensions/fuzzystrmatch.inference.toml`, once the schema runs
`CREATE EXTENSION fuzzystrmatch`.

`schema.sql`:

```sql
CREATE EXTENSION fuzzystrmatch;
CREATE TABLE users (id int PRIMARY KEY, name text NOT NULL, nick text);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Functions

`daitch_mokotoff` is NULL for a name without letters; the others only for a NULL argument.

```sql
SELECT soundex(name) AS code, metaphone(name, 4) AS meta, dmetaphone(nick) AS double, difference(name, 'Jon') AS diff, levenshtein(name, 'Jon') AS edits, levenshtein_less_equal(name, 'Jon', 2) AS bounded, daitch_mokotoff(name) AS codes FROM users
-- columns: code: str (TEXT)
-- columns: meta: str (TEXT)
-- columns: double: str | None (TEXT)
-- columns: diff: int (INT)
-- columns: edits: int (INT)
-- columns: bounded: int (INT)
-- columns: codes: list[str] | None (ARRAY<TEXT>)
-- rules: code: postgres.fuzzystrmatch.codes / postgres.fuzzystrmatch.codes
-- rules: meta: postgres.fuzzystrmatch.codes / postgres.fuzzystrmatch.codes
-- rules: double: postgres.fuzzystrmatch.codes / postgres.fuzzystrmatch.codes
-- rules: diff: postgres.fuzzystrmatch.distances / postgres.fuzzystrmatch.distances
-- rules: edits: postgres.fuzzystrmatch.distances / postgres.fuzzystrmatch.distances
-- rules: bounded: postgres.fuzzystrmatch.distances / postgres.fuzzystrmatch.distances
-- rules: codes: postgres.fuzzystrmatch.daitch-mokotoff / postgres.fuzzystrmatch.daitch-mokotoff
```
