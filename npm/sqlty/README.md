<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/sqlty-wordmark-dark.svg">
    <img src="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/sqlty-wordmark-light.svg" alt="SQLTy" width="300">
  </picture>
</h1>

Type-safe raw SQL for Python: row and parameter types inferred from your SQL and
schema, no ORM or database needed.

```console
npx sqlty check
```

This package runs the native `sqlty` executable built for your platform. It reads
your project the way the `sqlty` Python package does, with the project's virtual
environment (`VIRTUAL_ENV`, Conda, or the nearest `.venv`).

Documentation: <https://sqlty.kuncar.dev/>
