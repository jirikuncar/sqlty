# Magic Codec

## Simple integer query

```python
q = "--sql\nSELECT 1::INTEGER"
# decoded: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Multiple columns

```python
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## String without --sql prefix is unchanged

```python
q = "SELECT 1"
# decoded: q = "SELECT 1"
```

## Non-sql string is unchanged

```python
q = "just a normal string"
# decoded: q = "just a normal string"
```

## Datetime type triggers import

```python
q = "--sql\nSELECT created_at::TIMESTAMP"
# decoded: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP")
```

## UUID type triggers import

```python
q = "--sql\nSELECT user_id::UUID"
# decoded: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID")
```

## The exact example from the GitHub issue

```python
b'q = """--sql\nSELECT 1::INTEGER\n"""'
# decoded: b'q = """--sql\nSELECT 1::INTEGER\n"""'
```

## The example without explicit cast

```python
b'q = """--sql\nSELECT 1\n"""'
# decoded: b'q = """--sql\nSELECT 1\n"""'
```

## A complete file based on the issue example

```python
b'from sqlty import magic\n\nq = """--sql\nSELECT 1::INTEGER\n"""\n'
# decoded: b'from sqlty import magic\n\nq = """--sql\nSELECT 1::INTEGER\n"""\n'
```

## Demonstrate various SQL transformations (1)

```python
b'q = """--sql\nSELECT id::INTEGER, name::TEXT\n"""'
# decoded: b'q = """--sql\nSELECT id::INTEGER, name::TEXT\n"""'
```

## Demonstrate various SQL transformations (2)

```python
b'q = """--sql\nSELECT created_at::TIMESTAMP\n"""'
# decoded: b'q = """--sql\nSELECT created_at::TIMESTAMP\n"""'
```

## Triple-quoted strings keep their quotes

```python
a = """--sql
SELECT 1::INTEGER
"""
b = '''--sql
SELECT id::INTEGER, name::TEXT
'''
# decoded: a = SQL[tuple[int]]("""--sql
# decoded: SELECT 1::INTEGER
# decoded: """)
# decoded: b = SQL[tuple[int, str]]('''--sql
# decoded: SELECT id::INTEGER, name::TEXT
# decoded: ''')
```

## Whitespace before the marker

```python
q = """  --sql
SELECT 1::INTEGER
  """
# decoded: q = SQL[tuple[int]]("""  --sql
# decoded: SELECT 1::INTEGER
# decoded:   """)
```

## Invalid SQL is left alone

```python
q = "--sql\nNOT VALID SQL"
# decoded: q = "--sql\nNOT VALID SQL"
```
