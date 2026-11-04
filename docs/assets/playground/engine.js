// The sqlty engine compiled to WebAssembly (rust/crates/sqlty-wasm), with no DOM: the
// playground and tests/playground.test.mjs both load it.
export async function load(bytes) {
  const { instance } = await WebAssembly.instantiate(bytes);
  const { memory, buffer, run, output } = instance.exports;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  return (request) => {
    const encoded = encoder.encode(JSON.stringify(request));
    const at = buffer(encoded.length);
    new Uint8Array(memory.buffer, at, encoded.length).set(encoded);
    const length = run();
    const answer = output();
    return JSON.parse(decoder.decode(new Uint8Array(memory.buffer, answer, length)));
  };
}
