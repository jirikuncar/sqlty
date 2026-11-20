import type { Engine } from "./index.js";

/** Starts the engine from the bytes of `sqlty_wasm.wasm`, with no fetching or file access. */
export function load(bytes: BufferSource): Promise<Engine>;
