# Functions in the schema

`CREATE FUNCTION ... RETURNS` gives calls their type: a scalar, a `SETOF`, or the
columns of `RETURNS TABLE (...)`. A built-in function of the same name wins.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Scalar functions

`schema.sql`:

```sql
CREATE FUNCTION answer() RETURNS integer LANGUAGE sql AS 'SELECT 42';
CREATE FUNCTION full_name(first text, last text DEFAULT '') RETURNS text
    LANGUAGE sql AS $$ SELECT first || ' ' || last $$;
CREATE FUNCTION app.tags() RETURNS text[] LANGUAGE sql AS $$ SELECT ARRAY['a'] $$;
CREATE FUNCTION touch() RETURNS void LANGUAGE sql AS 'SELECT 1';
```

```sql
SELECT answer(), full_name('a'), full_name('a', 'b'), app.tags(), touch()
-- inferred: tuple[int | None, str | None, str | None, list[str] | None, None]
```

## Nullability

A function may return NULL whatever its arguments, so a call is nullable. A `STRICT`
function, also spelled `RETURNS NULL ON NULL INPUT`, returns NULL when an argument is
NULL: a call is nullable only when an argument is.

`schema.sql`:

```sql
CREATE TABLE t (a INTEGER, b INTEGER NOT NULL);
CREATE FUNCTION loose(x integer) RETURNS integer LANGUAGE sql AS 'SELECT x';
CREATE FUNCTION tight(x integer) RETURNS integer STRICT LANGUAGE sql AS 'SELECT x';
CREATE FUNCTION spelled(x integer) RETURNS integer RETURNS NULL ON NULL INPUT LANGUAGE sql AS 'SELECT x';
CREATE FUNCTION called(x integer) RETURNS integer CALLED ON NULL INPUT LANGUAGE sql AS 'SELECT x';
```

```sql
SELECT loose(b), tight(a), tight(b), spelled(b), called(b) FROM t
-- inferred: tuple[int | None, int | None, int, int, int | None]
```

## The call decides the overload

Overloads are told apart by their number of arguments. A call no overload accepts,
and one that fits overloads returning different types, stay unknown.

`schema.sql`:

```sql
CREATE FUNCTION f(a integer) RETURNS integer LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION f(a integer, b integer) RETURNS text LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION g(a integer) RETURNS integer LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION g(a text) RETURNS text LANGUAGE sql AS 'SELECT 1';
```

```sql
SELECT f(1), f(1, 2)
-- inferred: tuple[int | None, str | None]
```

```sql
SELECT f()
-- inferred: None
```

```sql
SELECT g(1)
-- inferred: None
```

## CREATE OR REPLACE

`schema.sql`:

```sql
CREATE FUNCTION answer() RETURNS integer LANGUAGE sql AS 'SELECT 42';
CREATE OR REPLACE FUNCTION answer() RETURNS bigint LANGUAGE sql AS 'SELECT 42';
```

```sql
SELECT answer()
-- inferred: tuple[int | None]
```

## DROP FUNCTION

`schema.sql`:

```sql
CREATE FUNCTION f(a integer) RETURNS integer LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION f(a text) RETURNS text LANGUAGE sql AS 'SELECT 1';
DROP FUNCTION f(text);
CREATE FUNCTION gone() RETURNS integer LANGUAGE sql AS 'SELECT 1';
DROP FUNCTION gone;
```

```sql
SELECT f(1)
-- inferred: tuple[int | None]
```

```sql
SELECT gone()
-- inferred: None
```

## Enum and domain results

`schema.sql`:

```sql
CREATE TYPE status AS ENUM ('new', 'done');
CREATE DOMAIN percent AS integer;
CREATE FUNCTION current_status() RETURNS status LANGUAGE sql AS $$ SELECT 'new'::status $$;
CREATE FUNCTION share() RETURNS percent LANGUAGE sql AS 'SELECT 1';
```

```sql
SELECT current_status(), share()
-- inferred: tuple[typing.Literal['new', 'done'] | None, int | None]
```

## Set-returning functions in FROM

`RETURNS TABLE`, `SETOF` a table and `OUT` parameters give the columns of the
row; `SETOF` a scalar type gives one column named like the function. A table's
row type without `SETOF` gives one row of the table's columns, each nullable: a
function that returns `NULL` gives a row of NULLs.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
CREATE FUNCTION active_users(since date) RETURNS TABLE (id integer, name text, seen timestamptz)
    LANGUAGE sql AS 'SELECT 1, ''a'', now()';
CREATE FUNCTION all_users() RETURNS SETOF users LANGUAGE sql AS 'SELECT * FROM users';
CREATE FUNCTION first_user() RETURNS users LANGUAGE sql AS 'SELECT * FROM users LIMIT 1';
CREATE FUNCTION ids() RETURNS SETOF integer LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION bounds(IN a integer, OUT lo integer, OUT hi integer)
    LANGUAGE sql AS 'SELECT a, a';
```

```sql
SELECT * FROM active_users('2024-01-01')
-- inferred: tuple[int | None, str | None, datetime.datetime | None]
```

```sql
SELECT u.id, u.seen FROM active_users('2024-01-01') AS u
-- inferred: tuple[int | None, datetime.datetime | None]
```

```sql
SELECT * FROM all_users()
-- inferred: tuple[int, str | None]
```

```sql
SELECT * FROM ids()
-- inferred: tuple[int]
```

```sql
SELECT * FROM first_user()
-- inferred: tuple[int | None, str | None]
```

```sql
SELECT u.id FROM first_user() AS u
-- inferred: tuple[int | None]
```

```sql
SELECT lo, hi FROM bounds(3)
-- inferred: tuple[int | None, int | None]
```

## Planner hints

`ROWS`, `COST`, `LEAKPROOF` and `SUPPORT` only guide the planner, so a function
with them keeps its type. `pg_dump` writes them after the language.

`schema.sql`:

```sql
CREATE FUNCTION accepted(ids integer[]) RETURNS SETOF integer
    LANGUAGE sql STABLE ROWS 1 PARALLEL SAFE
    AS $_$ SELECT x FROM unnest($1) AS x $_$;
CREATE FUNCTION admission(ids integer[]) RETURNS TABLE(admitted boolean)
    LANGUAGE sql STABLE COST 10 LEAKPROOF ROWS 1
    AS $_$ SELECT EXISTS (SELECT FROM accepted($1)) $_$;
```

```sql
SELECT * FROM accepted('{1}')
-- inferred: tuple[int]
```

```sql
SELECT a.admitted FROM admission('{1}') AS a
-- inferred: tuple[bool | None]
```

## A function whose definition sqlparser cannot read

A later definition that cannot be read leaves the function untyped, rather than
keeping the earlier return type.

`schema.sql`:

```sql
CREATE FUNCTION f() RETURNS integer LANGUAGE sql AS 'SELECT 1';
CREATE OR REPLACE FUNCTION f() RETURNS text LANGUAGE sql AS 'SELECT 1' SECURITY NONSENSE;
```

```sql
SELECT f()
-- inferred: None
```
