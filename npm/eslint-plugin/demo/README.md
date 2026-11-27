# Demo

```bash
make wasm                                   # from the repository root
(cd ../.. && npm ci)                        # the plugin with the engine just built
npx eslint .          # five problems in src/users.ts and src/orders.js
npx eslint . --fix    # writes the row types
```
