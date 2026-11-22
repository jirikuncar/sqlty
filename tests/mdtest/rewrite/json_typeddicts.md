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

## Keys with quotes, and names with no letters

A key is spelled as Python's `repr` writes it, and a class named after a column that starts
with a digit, or has no letters, is `Json` plus the rest.

```python
q = "--sql\nSELECT jsonb_build_object('it''s', name, 'a''\"b', id) AS doc FROM users"
r = "--sql\nSELECT jsonb_build_object('class', name) AS \"1x\" FROM users"
t = "--sql\nSELECT jsonb_build_object('class', name) AS \"__\" FROM users"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: Doc = typing.TypedDict('Doc', {"it's": str, 'a\'"b': int})
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: Json1x = typing.TypedDict('Json1x', {'class': str})
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: Json = typing.TypedDict('Json', {'class': str})
# rewritten-file:
# rewritten-file:
# rewritten-file: q = SQL[tuple[Doc]]("--sql\nSELECT jsonb_build_object('it''s', name, 'a''\"b', id) AS doc FROM users")
# rewritten-file: r = SQL[tuple[Json1x]]("--sql\nSELECT jsonb_build_object('class', name) AS \"1x\" FROM users")
# rewritten-file: t = SQL[tuple[Json]]("--sql\nSELECT jsonb_build_object('class', name) AS \"__\" FROM users")
```

## A statement that imports nothing binds no name

```python
from x
q = "--sql\nSELECT jsonb_build_object('class', name) AS doc FROM users"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file: from x
# rewritten-file: q = SQL[tuple[typing.Any]]("--sql\nSELECT jsonb_build_object('class', name) AS doc FROM users")
```

## A new class goes after the generated ones

```python
import typing


# sqlty: generated
B = typing.TypedDict('B', {})


x = 1
q = "--sql\nSELECT jsonb_build_object('class', name) AS doc FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: import typing
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: B = typing.TypedDict('B', {})
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: Doc = typing.TypedDict('Doc', {'class': str})
# rewritten-file:
# rewritten-file:
# rewritten-file: x = 1
# rewritten-file: q = SQL[tuple[Doc]]("--sql\nSELECT jsonb_build_object('class', name) AS doc FROM users")
```

## No free name is Any

When `Doc` and `Doc2` to `Doc99` are all taken, the column stays `typing.Any`.

```python
Doc = Doc2 = Doc3 = Doc4 = Doc5 = Doc6 = Doc7 = Doc8 = Doc9 = Doc10 = Doc11 = Doc12 = Doc13 = Doc14 = Doc15 = Doc16 = Doc17 = Doc18 = Doc19 = Doc20 = Doc21 = Doc22 = Doc23 = Doc24 = Doc25 = Doc26 = Doc27 = Doc28 = Doc29 = Doc30 = Doc31 = Doc32 = Doc33 = Doc34 = Doc35 = Doc36 = Doc37 = Doc38 = Doc39 = Doc40 = Doc41 = Doc42 = Doc43 = Doc44 = Doc45 = Doc46 = Doc47 = Doc48 = Doc49 = Doc50 = Doc51 = Doc52 = Doc53 = Doc54 = Doc55 = Doc56 = Doc57 = Doc58 = Doc59 = Doc60 = Doc61 = Doc62 = Doc63 = Doc64 = Doc65 = Doc66 = Doc67 = Doc68 = Doc69 = Doc70 = Doc71 = Doc72 = Doc73 = Doc74 = Doc75 = Doc76 = Doc77 = Doc78 = Doc79 = Doc80 = Doc81 = Doc82 = Doc83 = Doc84 = Doc85 = Doc86 = Doc87 = Doc88 = Doc89 = Doc90 = Doc91 = Doc92 = Doc93 = Doc94 = Doc95 = Doc96 = Doc97 = Doc98 = Doc99 = None
q = "--sql\nSELECT jsonb_build_object('class', name) AS doc FROM users"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file: Doc = Doc2 = Doc3 = Doc4 = Doc5 = Doc6 = Doc7 = Doc8 = Doc9 = Doc10 = Doc11 = Doc12 = Doc13 = Doc14 = Doc15 = Doc16 = Doc17 = Doc18 = Doc19 = Doc20 = Doc21 = Doc22 = Doc23 = Doc24 = Doc25 = Doc26 = Doc27 = Doc28 = Doc29 = Doc30 = Doc31 = Doc32 = Doc33 = Doc34 = Doc35 = Doc36 = Doc37 = Doc38 = Doc39 = Doc40 = Doc41 = Doc42 = Doc43 = Doc44 = Doc45 = Doc46 = Doc47 = Doc48 = Doc49 = Doc50 = Doc51 = Doc52 = Doc53 = Doc54 = Doc55 = Doc56 = Doc57 = Doc58 = Doc59 = Doc60 = Doc61 = Doc62 = Doc63 = Doc64 = Doc65 = Doc66 = Doc67 = Doc68 = Doc69 = Doc70 = Doc71 = Doc72 = Doc73 = Doc74 = Doc75 = Doc76 = Doc77 = Doc78 = Doc79 = Doc80 = Doc81 = Doc82 = Doc83 = Doc84 = Doc85 = Doc86 = Doc87 = Doc88 = Doc89 = Doc90 = Doc91 = Doc92 = Doc93 = Doc94 = Doc95 = Doc96 = Doc97 = Doc98 = Doc99 = None
# rewritten-file: q = SQL[tuple[typing.Any]]("--sql\nSELECT jsonb_build_object('class', name) AS doc FROM users")
```
