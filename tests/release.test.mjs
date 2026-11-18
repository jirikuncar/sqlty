// The playground's released engines (docs/assets/playground/release.js), with the registry
// answering from an `npm pack` of npm/wasm: `make playground`, then
// `node --test tests/release.test.mjs`.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "node:test";

import { entry, released, versions } from "../docs/assets/playground/release.js";

const REGISTRY = "https://registry.npmjs.org/@sqlty/wasm";
const TARBALL = `${REGISTRY}/-/wasm-1.2.3.tgz`;
const directory = mkdtempSync(join(tmpdir(), "sqlty-release-"));
const [packed] = JSON.parse(
  execFileSync("npm", ["pack", "--json", "--pack-destination", directory], {
    cwd: new URL("../npm/wasm/", import.meta.url),
  }),
);
const tarball = readFileSync(join(directory, packed.filename));
const wasm = readFileSync(new URL("../npm/wasm/sqlty_wasm.wasm", import.meta.url));
const integrity = `sha512-${createHash("sha512").update(tarball).digest("base64")}`;

const { fetch } = globalThis;
afterEach(() => (globalThis.fetch = fetch));

function registry(answers) {
  globalThis.fetch = async (url) => {
    const found = answers[url];
    if (found === undefined) return new Response("", { status: 404 });
    return new Response(found instanceof Uint8Array ? found : JSON.stringify(found));
  };
}

test("the releases, newest first, without placeholders and pre-releases", async () => {
  const listed = ["0.0.0", "0.9.0", "0.10.0", "0.10.1-rc.1", "0.8.2"];
  registry({
    [REGISTRY]: {
      versions: Object.fromEntries(
        listed.map((version) => [version, { version, ...(version === "0.0.0" && { deprecated: "placeholder" }) }]),
      ),
    },
  });
  assert.deepEqual(await versions(), ["0.10.0", "0.9.0", "0.8.2"]);
});

test("a release is the module of its tarball", async () => {
  registry({ [`${REGISTRY}/1.2.3`]: { dist: { tarball: TARBALL, integrity } }, [TARBALL]: tarball });
  assert.deepEqual(Buffer.from(await released("1.2.3")), wasm);
});

test("a tarball the registry does not list is refused", async () => {
  registry({ [`${REGISTRY}/1.2.3`]: { dist: { tarball: TARBALL, integrity: "sha512-x" } }, [TARBALL]: tarball });
  await assert.rejects(released("1.2.3"), { message: "@sqlty/wasm@1.2.3 is not the published tarball" });
});

test("a missing release or tarball, or no version, is an error", async () => {
  registry({ [`${REGISTRY}/1.2.3`]: { dist: { tarball: TARBALL, integrity } } });
  await assert.rejects(released("1.2.3"), { message: `${TARBALL}: 404` });
  await assert.rejects(released("9.9.9"), { message: `${REGISTRY}/9.9.9: 404` });
  await assert.rejects(released("../latest"), { message: "../latest is not a release" });
});

test("a file a tar archive lacks", () => {
  const tar = new Uint8Array(1024);
  tar.set(new TextEncoder().encode("package/other"), 0);
  tar.set(new TextEncoder().encode("0"), 124);
  tar.set(new TextEncoder().encode("package"), 345);
  assert.throws(() => entry(tar, "package/sqlty_wasm.wasm"), { message: "package/sqlty_wasm.wasm is not in the package" });
});
