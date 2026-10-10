# SQLTy Documentation

Every Python and SQL example in these pages is executed by the test suite,
on both backends: what you read is what sqlty does.

## Start

- [Motivation](motivation.md): the problem, and when sqlty fits
- [Installation](installation.md)
- [Quick start](guide/quick-start.md): write, rewrite, check, strip

## Guide

- [Schema inference](guide/schema-inference.md): types from `CREATE TABLE`
- [User-defined row types](guide/user-types.md): `TypedDict`, `NamedTuple`, dataclass, Pydantic
- [Configuration](guide/configuration.md): `[tool.sqlty]`, dialects, severities, pre-commit
- [Import style](guide/import-style.md): `Q[T]`, `sqlty.SQL[T]`
- [Untyped queries](guide/untyped.md): the `UNTYPED` sentinel
- [psycopg 3](guide/psycopg.md): rows typed from `SQL[T]`
- [Magic codec](guide/magic-codec.md): rewrite at import time

## Reference

- [Lint rules](rules.md), one page per rule
- [CLI](reference/cli.md)
- [Python API](reference/api.md)
- [Type mappings](reference/type-mappings.md)

Contributing: [DEVELOPMENT.md](../DEVELOPMENT.md).
