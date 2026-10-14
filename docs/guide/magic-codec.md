# Magic codec

The magic codec rewrites `--sql` strings when Python reads a source file, so
the module runs with `SQL[T](...)` wrappers without a `sqlty rewrite` step.
It is meant for scripts, notebooks and prototypes. For code you commit, run
[`sqlty rewrite`](../quickstart.md) so type checkers see the types too: they
read the file on disk, not the decoded source.

## Usage

Declare the `sqlty` source encoding on the first or second line of the file:

```python
# coding: sqlty

query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

The `# decoded:` line shows the code Python runs. The codec wraps strings as
`sqlty rewrite` does and adds the imports the inferred types need
(`from sqlty import SQL`, `import datetime`, `import uuid`, `import decimal`, `import ipaddress`, `import typing`) after the module
docstring and `__future__` imports. They go on an existing line, so line
numbers in tracebacks match the file on disk, unless a `def`, `class`,
decorator or other compound statement directly follows the coding line; put a
docstring, blank line or comment between them to keep the numbers the same:

```python
# coding: sqlty

query = "--sql\nSELECT created_at::TIMESTAMP, token::UUID FROM sessions"
# decoded: query = SQL[tuple[datetime.datetime, uuid.UUID]]("--sql\nSELECT created_at::TIMESTAMP, token::UUID FROM sessions")
```

Strings the rewriter would leave alone stay unchanged: invalid SQL, DML
without `RETURNING`, DDL, empty queries and f-/t-strings.

```python
# coding: sqlty

purge = "--sql\nDELETE FROM sessions WHERE expired"
# decoded: purge = "--sql\nDELETE FROM sessions WHERE expired"
```

## Registering the codec

Python looks up the `sqlty` encoding when it compiles the file, so the codec
must be registered first. Installing sqlty (`pip install sqlty`,
`uv add sqlty`, or building it from the sdist) adds a
[`sqlty_magic.pth`](https://github.com/jirikuncar/sqlty/blob/main/src/sqlty_magic.pth)
file to site-packages. Python runs its one line in every interpreter that
uses that environment: it registers a lookup that answers only the `sqlty`
encoding and imports `sqlty.magic` the first time a file declares it, so
startup does not import sqlty.

Editable installs (`uv sync` or `pip install -e .` inside a sqlty checkout,
`maturin develop`) do not add the `.pth` file. There, or wherever the hook is
disabled, import `sqlty.magic` before the first module that uses the codec:

```python
import sqlty.magic  # noqa: F401  registers the "sqlty" codec

import queries  # queries.py starts with "# coding: sqlty"
```

Without registration, a file declaring `# coding: sqlty` fails with
`SyntaxError: unknown encoding: sqlty`. Files without the declaration are
never transformed, even with the codec registered.

Set `SQLTY_NO_MAGIC` to any non-empty value (even `0`) to skip registration,
both in the `.pth` hook at startup and when `sqlty.magic` is imported, for
example to turn the hook off for one run:

```bash
SQLTY_NO_MAGIC=1 python script.py
```

A `# coding: sqlty` file can be imported, run with `python -m module`, or run
directly as a script (`python script.py`).

## Inspecting the output

Print the decoded source of a file without running it:

```bash
python -m sqlty.magic queries.py
```

The output declares `# coding: utf-8` instead of `sqlty`, so it runs without
the codec.

## Configuration

The codec uses the PostgreSQL dialect and no schema, so cast every column;
a query with an uncast column is not typed. It reads only `import-style` from the
`pyproject.toml` found from the current working directory (see
[Configuration](../reference/configuration.md#import-style)).

## Example

[`examples/magic_codec/`](https://github.com/jirikuncar/sqlty/tree/main/examples/magic_codec)
contains a runnable module that uses the codec.
