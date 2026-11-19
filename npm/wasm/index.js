import { load as start } from "./engine.js";

export async function load(wasm = new URL("./sqlty_wasm.wasm", import.meta.url)) {
  return start(await bytes(await wasm));
}

async function bytes(wasm) {
  if (typeof wasm === "string" || wasm instanceof URL) {
    const url = new URL(wasm, globalThis.location?.href);
    const fs = url.protocol === "file:" && globalThis.process?.getBuiltinModule?.("node:fs/promises");
    return fs ? fs.readFile(url) : bytes(await fetch(url));
  }
  if (wasm instanceof Response) {
    if (!wasm.ok) throw new Error(`${wasm.url || "sqlty_wasm.wasm"}: ${wasm.status} ${wasm.statusText}`);
    return wasm.arrayBuffer();
  }
  return wasm;
}
