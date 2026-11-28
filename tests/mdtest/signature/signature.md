# Signature help

Signature help shows the overloads of the built-in function whose arguments the cursor is
in, from the catalog of the dialect, and highlights the parameter the argument binds to. The
`<CURSOR>` marks where the cursor is; `# signature:` lists the overloads, the active one
after `> ` with its parameter in `**`, `None` for none.

## Overloads

The active overload is the first that takes as many arguments as the call has so far.

```python
q = "--sql\nSELECT date_trunc('day', <CURSOR>) FROM orders"
# signature: > date_trunc(text, **timestamp**) -> timestamp
# signature: date_trunc(text, timestamp with time zone, text) -> timestamp with time zone
# signature: date_trunc(text, interval) -> interval
```

```python
q = "--sql\nSELECT date_trunc('day', created, <CURSOR>) FROM orders"
# signature: date_trunc(text, timestamp) -> timestamp
# signature: > date_trunc(text, timestamp with time zone, **text**) -> timestamp with time zone
# signature: date_trunc(text, interval) -> interval
```

Before its first argument, a call shows the first overload.

```python
q = "--sql\nSELECT date_trunc(<CURSOR>) FROM orders"
# signature: > date_trunc(**text**, timestamp) -> timestamp
# signature: date_trunc(text, timestamp with time zone, text) -> timestamp with time zone
# signature: date_trunc(text, interval) -> interval
```

## Optional and repeated parameters

```python
q = "--sql\nSELECT ltrim(name<CURSOR>) FROM users"
# signature: > ltrim(**string: text**[, characters: text]) -> text
# signature: ltrim(bytes: bytea, bytesremoved: bytea) -> bytea
```

A repeated parameter stays active for every later argument.

```python
q = "--sql\nSELECT concat_ws(', ', first, last, <CURSOR>) FROM users"
# signature: > concat_ws(sep: text, val1: "any"[, **val2: "any"**, ...]) -> text
```

```python
q = "--sql\nSELECT coalesce(nickname, name, <CURSOR>) FROM users"
# signature: > coalesce(**value: anyelement**, ...) -> anyelement
```

## Named arguments

An argument passed by name makes its parameter active; after it, positions say nothing.

```python
q = "--sql\nSELECT make_interval(days => <CURSOR>)"
# signature: > make_interval([years: int[, months: int[, weeks: int[, **days: int**[, hours: int[, mins: int[, secs: double precision]]]]]]]) -> interval
```

```python
q = "--sql\nSELECT make_interval(days => 1, <CURSOR>)"
# signature: > make_interval([years: int[, months: int[, weeks: int[, days: int[, hours: int[, mins: int[, secs: double precision]]]]]]]) -> interval
```

## Aggregates

```python
q = "--sql\nSELECT max(<CURSOR>) FROM orders"
# signature: > max(**anyelement**) -> anyelement
```

## Too many arguments

No overload takes the argument: the first is shown, with no parameter.

```python
q = "--sql\nSELECT left(name, 3, <CURSOR>) FROM users"
# signature: > left(string: text, n: integer) -> text
```

## Nested calls

The innermost call answers.

```python
q = "--sql\nSELECT upper(left(name, <CURSOR>)) FROM users"
# signature: > left(string: text, **n: integer**) -> text
```

```python
q = "--sql\nSELECT upper(left(name, 3)<CURSOR>) FROM users"
# signature: > upper(**text**) -> text
# signature: upper(anyrange) -> anyelement
# signature: upper(anymultirange) -> anyelement
```

## Case and qualified names

```python
q = "--sql\nSELECT pg_catalog.UPPER(<CURSOR>) FROM users"
# signature: > upper(**text**) -> text
# signature: upper(anyrange) -> anyelement
# signature: upper(anymultirange) -> anyelement
```

A quoted name matches only as the catalog spells it.

```python
q = '--sql\nSELECT "upper"(<CURSOR>) FROM users'
# signature: > upper(**text**) -> text
# signature: upper(anyrange) -> anyelement
# signature: upper(anymultirange) -> anyelement
```

```python
q = '--sql\nSELECT "UPPER"(<CURSOR>) FROM users'
# signature: None
```

## No call

Grouping parentheses, a subquery, a column list and a string answer nothing.

```python
q = "--sql\nSELECT (price + <CURSOR>) FROM orders"
# signature: None
```

```python
q = "--sql\nSELECT upper((<CURSOR>)) FROM users"
# signature: None
```

```python
q = "--sql\nSELECT id FROM users WHERE id IN (<CURSOR>)"
# signature: None
```

```python
q = "--sql\nINSERT INTO point (<CURSOR>) VALUES (1)"
# signature: None
```

```python
q = "--sql\nINSERT INTO public.point (<CURSOR>) VALUES (1)"
# signature: None
```

```python
q = "--sql\nSELECT upper('a, <CURSOR>') FROM users"
# signature: None
```

```python
q = "--sql\nSELECT name<CURSOR> FROM users"
# signature: None
```

## A function the catalog does not list

```python
q = "--sql\nSELECT my_function(<CURSOR>) FROM users"
# signature: None
```

## Dialects without a catalog

```toml
[tool.sqlty]
dialect = "clickhouse"
```

```python
q = "--sql\nSELECT upper(<CURSOR>) FROM users"
# signature: None
```
