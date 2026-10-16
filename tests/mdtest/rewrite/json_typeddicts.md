# Generated TypedDicts for JSON columns

With `json-types = "typing.TypedDict"`, `rewrite` types a JSON column of known shape with
generated classes instead of `typing.Any`.

```toml
[tool.sqlty]
json-types = "typing.TypedDict"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT, prefs JSONB);
```

## Objects, arrays, maps and optional members

```python
q = "--sql\nSELECT jsonb_build_object('user', jsonb_build_object('id', id, 'name', name), 'tags', jsonb_build_array('a'), 'counts', jsonb_object_agg(name, id)) AS doc FROM users GROUP BY id, name"
r = "--sql\nSELECT jsonb_build_object('id', id) AS item FROM users UNION ALL SELECT jsonb_build_object('id', id, 'email', email) FROM users"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class DocUser(typing.TypedDict):
# rewritten-file:     id: int
# rewritten-file:     name: str
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class Doc(typing.TypedDict):
# rewritten-file:     user: DocUser
# rewritten-file:     tags: list[str]
# rewritten-file:     counts: dict[str, int]
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class Item(typing.TypedDict):
# rewritten-file:     id: int
# rewritten-file:     email: typing.NotRequired[str | None]
# rewritten-file:
# rewritten-file:
# rewritten-file: q = SQL[tuple[Doc]]("--sql\nSELECT jsonb_build_object('user', jsonb_build_object('id', id, 'name', name), 'tags', jsonb_build_array('a'), 'counts', jsonb_object_agg(name, id)) AS doc FROM users GROUP BY id, name")
# rewritten-file: r = SQL[tuple[Item]]("--sql\nSELECT jsonb_build_object('id', id) AS item FROM users UNION ALL SELECT jsonb_build_object('id', id, 'email', email) FROM users")
```

## Keys that are not names

```python
q = "--sql\nSELECT jsonb_build_object('first-name', name, 'class', 1) AS doc FROM users"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: Doc = typing.TypedDict('Doc', {'first-name': str, 'class': int})
# rewritten-file:
# rewritten-file:
# rewritten-file: q = SQL[tuple[Doc]]("--sql\nSELECT jsonb_build_object('first-name', name, 'class', 1) AS doc FROM users")
```

## A JSON column with no shape stays Any

```python
q = "--sql\nSELECT id, prefs FROM users"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int, typing.Any]]("--sql\nSELECT id, prefs FROM users")
```

## Names in use get a number, generated classes are reused

```python
import typing

from sqlty import SQL


class Item:
    pass


# sqlty: generated
class Doc(typing.TypedDict):
    id: int


q = SQL[tuple[Doc]]("--sql\nSELECT jsonb_build_object('id', id) AS doc FROM users")
r = "--sql\nSELECT jsonb_build_object('id', id) AS doc, jsonb_build_object('name', name) AS item FROM users"
# rewritten-file: import typing
# rewritten-file:
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: class Item:
# rewritten-file:     pass
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class Doc(typing.TypedDict):
# rewritten-file:     id: int
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class Item2(typing.TypedDict):
# rewritten-file:     name: str
# rewritten-file:
# rewritten-file:
# rewritten-file: q = SQL[tuple[Doc]]("--sql\nSELECT jsonb_build_object('id', id) AS doc FROM users")
# rewritten-file: r = SQL[tuple[Doc, Item2]]("--sql\nSELECT jsonb_build_object('id', id) AS doc, jsonb_build_object('name', name) AS item FROM users")
```

## Strip removes the generated classes

```python
import typing

from sqlty import SQL


# sqlty: generated
class Doc(typing.TypedDict):
    id: int


q = SQL[tuple[Doc]]("--sql\nSELECT jsonb_build_object('id', id) AS doc FROM users")
# stripped: q = "--sql\nSELECT jsonb_build_object('id', id) AS doc FROM users"
```

## check --fix types the strings it wraps

```python
q = "--sql\nSELECT jsonb_agg(jsonb_build_object('id', id)) AS ids FROM users"
# fixed: import typing
# fixed: from sqlty import SQL
# fixed:
# fixed:
# fixed: # sqlty: generated
# fixed: class IdsItem(typing.TypedDict):
# fixed:     id: int
# fixed:
# fixed:
# fixed: q = SQL[tuple[list[IdsItem] | None]]("--sql\nSELECT jsonb_agg(jsonb_build_object('id', id)) AS ids FROM users")
```
