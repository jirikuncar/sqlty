# ltree

Paths in a tree, from `rules/postgres/extensions/ltree.inference.toml`, once the schema runs
`CREATE EXTENSION ltree`. `ltree`, `lquery` and `ltxtquery` values load as `str`.

`schema.sql`:

```sql
CREATE EXTENSION ltree;
CREATE TABLE nodes (id int PRIMARY KEY, path ltree NOT NULL, parent ltree);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Functions

```sql
SELECT subpath(path, 0, 1) AS head, subltree(parent, 0, 1) AS top, nlevel(path) AS depth, index(path, 'b'::ltree) AS at, ltree2text(path) AS text, text2ltree('a.b') AS made, lca(path, parent) AS common FROM nodes
-- columns: head: str (LTREE)
-- columns: top: str | None (LTREE)
-- columns: depth: int (INT)
-- columns: at: int (INT)
-- columns: text: str (TEXT)
-- columns: made: str (LTREE)
-- columns: common: str | None (LTREE)
-- rules: head: postgres.ltree.path / postgres.ltree.path
-- rules: top: postgres.ltree.path / postgres.ltree.path
-- rules: depth: postgres.ltree.position / postgres.ltree.position
-- rules: at: postgres.ltree.position / postgres.ltree.position
-- rules: text: postgres.ltree.text / postgres.ltree.text
-- rules: made: postgres.ltree.path / postgres.ltree.path
-- rules: common: postgres.ltree.ancestor-of-all / postgres.ltree.ancestor-of-all
```

## Operators

`@>` and `<@` test ancestry, `~` matches an `lquery` and `||` joins paths.

```sql
SELECT path @> parent AS above, path <@ 'a'::ltree AS below, path ~ '*.b.*'::lquery AS matches, path || 'c'::ltree AS child, path || 'c'::text AS named, 'root'::text || path AS rooted FROM nodes
-- columns: above: bool | None (BOOLEAN)
-- columns: below: bool (BOOLEAN)
-- columns: matches: bool (BOOLEAN)
-- columns: child: str (LTREE)
-- columns: named: str (LTREE)
-- columns: rooted: str (LTREE)
-- rules: above: postgres.ltree.ancestor / postgres.ltree.ancestor
-- rules: below: postgres.ltree.ancestor / postgres.ltree.ancestor
-- rules: matches: postgres.ltree.match / postgres.ltree.match
-- rules: child: postgres.ltree.concat / postgres.ltree.concat
-- rules: named: postgres.ltree.concat / postgres.ltree.concat
-- rules: rooted: postgres.ltree.concat-text / postgres.ltree.concat-text
```

```sql
SELECT id FROM nodes WHERE path <@ $1 AND $2 @> path AND path ~ $3
-- params: 1: str (LTREE)
-- params: 2: str (LTREE)
-- params: 3: str (LQUERY)
```

## Without `CREATE EXTENSION`

`schema.sql`:

```sql
CREATE TABLE nodes (id int PRIMARY KEY, label text NOT NULL);
```

```sql
SELECT nlevel(label) AS depth FROM nodes
-- columns: depth: None (None)
```
