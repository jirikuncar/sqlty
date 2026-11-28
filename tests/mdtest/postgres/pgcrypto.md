# pgcrypto

Password hashing and encryption, from `rules/postgres/extensions/pgcrypto.inference.toml`,
once the schema runs `CREATE EXTENSION pgcrypto`.

`schema.sql`:

```sql
CREATE EXTENSION pgcrypto;
CREATE TABLE users (id int PRIMARY KEY, password text NOT NULL, secret bytea);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Functions

```sql
SELECT crypt('x', gen_salt('bf')) AS hash, hmac('x', 'key', 'sha256') AS mac, pgp_sym_encrypt('x', 'key') AS sealed, armor(secret) AS armored, dearmor(armor('\x00'::bytea)) AS raw FROM users
-- columns: hash: str (TEXT)
-- columns: mac: bytes (VARBINARY)
-- columns: sealed: bytes (VARBINARY)
-- columns: armored: str | None (TEXT)
-- columns: raw: bytes (VARBINARY)
-- rules: hash: postgres.pgcrypto.text / postgres.pgcrypto.text
-- rules: mac: postgres.pgcrypto.bytes / postgres.pgcrypto.bytes
-- rules: sealed: postgres.pgcrypto.bytes / postgres.pgcrypto.bytes
-- rules: armored: postgres.pgcrypto.text / postgres.pgcrypto.text
-- rules: raw: postgres.pgcrypto.bytes / postgres.pgcrypto.bytes
```

`crypt` with a stored hash and `pgp_sym_decrypt` raise an error for a wrong salt or key:

```sql
SELECT crypt($1, password) = password AS ok, pgp_sym_decrypt(secret, $2) AS opened FROM users
-- columns: ok: bool (BOOLEAN)
-- columns: opened: str | None (TEXT)
-- rules: ok: common.comparison / common.comparison
-- rules: opened: postgres.pgcrypto.text / postgres.pgcrypto.text
```

## Without `CREATE EXTENSION`

`schema.sql`:

```sql
CREATE TABLE users (id int PRIMARY KEY, password text NOT NULL);
```

`digest` and `gen_random_bytes` are typed without the extension too, as before sqlty knew
pgcrypto; `crypt` is not:

```sql
SELECT digest(password, 'sha256') AS hashed, gen_random_bytes(16) AS salt, crypt('x', password) AS hash FROM users
-- columns: hashed: bytes (VARBINARY)
-- columns: salt: bytes (VARBINARY)
-- columns: hash: None (None)
```
