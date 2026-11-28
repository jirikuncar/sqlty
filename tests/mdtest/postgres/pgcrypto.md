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
SELECT crypt('x', gen_salt('bf')) AS hash, crypt('x', password) = password AS ok, hmac('x', 'key', 'sha256') AS mac, pgp_sym_encrypt('x', 'key') AS sealed, pgp_sym_decrypt(secret, 'key') AS opened, armor(secret) AS armored, dearmor(armor('\x00'::bytea)) AS raw FROM users
-- columns: hash: str (TEXT)
-- columns: ok: bool (BOOLEAN)
-- columns: mac: bytes (VARBINARY)
-- columns: sealed: bytes (VARBINARY)
-- columns: opened: str | None (TEXT)
-- columns: armored: str | None (TEXT)
-- columns: raw: bytes (VARBINARY)
-- rules: hash: postgres.pgcrypto.text / postgres.pgcrypto.text
-- rules: ok: common.comparison / common.comparison
-- rules: mac: postgres.pgcrypto.bytes / postgres.pgcrypto.bytes
-- rules: sealed: postgres.pgcrypto.bytes / postgres.pgcrypto.bytes
-- rules: opened: postgres.pgcrypto.text / postgres.pgcrypto.text
-- rules: armored: postgres.pgcrypto.text / postgres.pgcrypto.text
-- rules: raw: postgres.pgcrypto.bytes / postgres.pgcrypto.bytes
```

`digest` and `gen_random_bytes` are typed without the extension too:

```sql
SELECT digest(password, 'sha256') AS hashed, gen_random_bytes(16) AS salt FROM users
-- columns: hashed: bytes (VARBINARY)
-- columns: salt: bytes (VARBINARY)
```
