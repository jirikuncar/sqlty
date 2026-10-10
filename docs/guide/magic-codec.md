# Magic Codec

The `sqlty` source codec rewrites `--sql` strings when a module is imported,
so there is no `sqlty rewrite` step. Useful while prototyping; commit
rewritten files for type checkers, which never run the codec. Runnable
example: [`examples/magic_codec/`](../../examples/magic_codec/).

## Usage

Declare the codec on the first or second line of the module:

```python
# coding: sqlty
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: # coding: sqlty
# decoded: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Installing sqlty adds `sqlty_magic.pth` to `site-packages`, which registers
the codec at interpreter startup. `SQLTY_NO_MAGIC=1` disables the
registration. Files without the declaration are not transformed.

## Imports

`datetime` and `uuid` imports are added when the row type needs them:

```python
# coding: sqlty
q = "--sql\nSELECT created_at::TIMESTAMP, user_id::UUID FROM events"
# decoded: # coding: sqlty
# decoded:
# decoded:
# decoded: q = SQL[tuple[datetime.datetime, uuid.UUID]]("--sql\nSELECT created_at::TIMESTAMP, user_id::UUID FROM events")
```

## Untypeable strings stay strings

```python
# coding: sqlty
ddl = "--sql\nCREATE TABLE t (id INTEGER)"
plain = "SELECT 1"
# decoded: # coding: sqlty
# decoded: ddl = "--sql\nCREATE TABLE t (id INTEGER)"
# decoded: plain = "SELECT 1"
```

## Without registration

`sqlty._decode.decode` transforms bytes without touching the codec registry,
and `python -m sqlty.magic module.py` prints a module as Python sees it.

```python
from sqlty._decode import decode

source, _ = decode(b'q = "--sql\\nSELECT 1::INTEGER"')
```
