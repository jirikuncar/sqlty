<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/sqlty-wordmark-dark.svg">
    <img src="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/sqlty-wordmark-light.svg" alt="SQLTy" width="300">
  </picture>
</h1>

The native `sqlty` executable for SYSTEM. Install the
[`@sqlty/cli`](https://www.npmjs.com/package/@sqlty/cli) package instead: it picks this one
for your platform.

sqlty types raw SQL in Python: row and parameter types inferred from your SQL and
schema, no ORM or database needed.

```console
npx @sqlty/cli check     # check the SQL in your Python strings against the schema
npx @sqlty/cli rewrite   # wrap query strings as SQL[T](...) with their row types
npx @sqlty/cli server    # the language server, for editors
```

Documentation: <https://sqlty.kuncar.dev/>
