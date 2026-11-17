import assert from "node:assert/strict";
import { test } from "node:test";

import { encode, session, toml } from "../../playground";

test("a session keeps the keys the playground takes", () => {
  const settings = { dialect: "sqlite", driver: "psycopg", schema: "db.sql", rules: { a: "warn" } };
  assert.deepEqual(session("x = 1", { settings, schema: "CREATE TABLE t (a int);" }), {
    source: "x = 1",
    schema: "CREATE TABLE t (a int);",
    dialect: "sqlite",
    config: 'driver = "psycopg"\n\n[rules]\na = "warn"\n',
  });
  assert.equal(session("", { settings: { dialect: 1 }, schema: "" }).dialect, "postgres");
});

test("toml writes scalars, then tables, quoting keys that need it", () => {
  const table = { "max-query-variants": 4, preview: true, types: { "a.b": { x: ["s", 1] } } };
  assert.equal(
    toml(table),
    'max-query-variants = 4\npreview = true\n\n[types]\n[types."a.b"]\nx = ["s", 1]\n',
  );
});

test("encode deflates the JSON of a session as base64url", async () => {
  const value = session("?".repeat(300) + "ÿ", { settings: {}, schema: "" });
  const encoded = await encode(value);
  assert.match(encoded, /^[\w-]+$/);
  const bytes = Uint8Array.from(atob(encoded.replaceAll("-", "+").replaceAll("_", "/")), (c) =>
    c.charCodeAt(0),
  );
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("deflate"));
  assert.deepEqual(JSON.parse(await new Response(stream).text()), value);
});
