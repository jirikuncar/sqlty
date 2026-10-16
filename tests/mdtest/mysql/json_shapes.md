# MySQL JSON shapes

`JSON_OBJECT`, `JSON_ARRAY`, `JSON_ARRAYAGG` and `JSON_OBJECTAGG` build JSON of a
known shape, checked by `json-shape-mismatch` like PostgreSQL's builders. MySQL
has no boolean type: a `BOOLEAN` column is a `TINYINT` and becomes a JSON
number; only `TRUE` and `FALSE` are JSON booleans.

`schema.sql`:

```sql
CREATE TABLE users (id INT NOT NULL PRIMARY KEY, name VARCHAR(20), active BOOLEAN NOT NULL, born DATE, score DECIMAL(5, 2) NOT NULL);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## Shapes

```sql
SELECT JSON_OBJECT('id', id, 'name', name, 'active', active, 'flag', TRUE, 'born', born, 'score', score) AS doc FROM users
-- json: doc: {id: integer, name: string | null, active: integer, flag: boolean, born: string | null, score: number}
```

```sql
SELECT JSON_ARRAY(id, score) AS pair FROM users
-- json: pair: [number]
```

```sql
SELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'name', name)) AS items, JSON_OBJECTAGG(id, score) AS scores FROM users
-- json: items: [{id: integer, name: string | null}]
-- json: scores: {[key]: number}
```

## json-shape-mismatch

```python
from typing import TypedDict
from sqlty import SQL

class Doc(TypedDict):
    id: int
    name: str

class Row(TypedDict):
    doc: Doc

class Items(TypedDict):
    items: list[Doc]

# error: [json-shape-mismatch] "Row.doc is Doc but query column 'doc' builds JSON that does not fit: doc.name can be null but is declared str"
row = SQL[Row]("-- sql\nSELECT JSON_OBJECT('id', id, 'name', name) AS doc FROM users")
# error: [nullable-mismatch] "Items.items is list[Any] but query column 'items' can be NULL"
items = SQL[Items]("-- sql\nSELECT JSON_ARRAYAGG(JSON_OBJECT('id', id, 'name', COALESCE(name, ''))) AS items FROM users")
```
