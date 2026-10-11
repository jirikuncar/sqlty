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

The `# decoded:` line shows the code Python runs. The codec adds the imports
the inferred types need (`from sqlty import SQL`, `import datetime`,
`import uuid`):

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
must be registered first. Installing sqlty from a wheel (`pip install sqlty`,
`uv add sqlty`) adds a `sqlty_magic.pth` file to site-packages that imports
`sqlty.magic` on every interpreter start, so the codec is registered before
your code runs.

Editable installs of a sqlty checkout (`uv sync`, `maturin develop`) do not
add the `.pth` file. There, or wherever the hook is disabled, import
`sqlty.magic` before the first module that uses the codec:

```python
import sqlty.magic  # noqa: F401  registers the "sqlty" codec

import queries  # queries.py starts with "# coding: sqlty"
```

Without registration, a file declaring `# coding: sqlty` fails with
`SyntaxError: unknown encoding: sqlty`. Files without the declaration are
never transformed, even with the codec registered.

Set `SQLTY_NO_MAGIC` to any non-empty value (even `0`) before `sqlty.magic`
is imported to skip registration, for example to turn the `.pth` hook off for
one run:

```bash
SQLTY_NO_MAGIC=1 python script.py
```

## Inspecting the output

Print the decoded source of a file without running it:

```bash
python -m sqlty.magic queries.py
```

## Configuration

The codec uses the PostgreSQL dialect and no schema, so cast every column;
an uncast column is typed `str`. It reads only `import-style` from the
`pyproject.toml` found from the current working directory (see
[Configuration](../reference/configuration.md#import-style)).

## Example

[`examples/magic_codec/`](https://github.com/jirikuncar/sqlty/tree/main/examples/magic_codec)
contains a runnable module that uses the codec.
