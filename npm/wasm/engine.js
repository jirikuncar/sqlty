// The sqlty engine compiled to WebAssembly (rust/crates/sqlty-wasm), started from the bytes
// of `sqlty_wasm.wasm`: the docs playground, the VS Code extension and `index.js` load it.
//
// A panic traps and leaves the instance unusable: a spare instance answers the request again
// with what it panicked with, which `analyze` reports as `internal-error`. The browser
// instantiates a module this large only asynchronously, so the spare is made in advance.
export async function load(bytes) {
  const module = await WebAssembly.compile(bytes);
  const start = () => WebAssembly.instantiate(module);
  let [instance, spare] = await Promise.all([start(), start()]);
  const answer = (request) => {
    if (!instance) {
      if (!spare) throw new Error("sqlty crashed and is starting again");
      [instance, spare] = [spare, undefined];
      start().then((fresh) => (spare = fresh));
    }
    try {
      return ask(instance, request);
    } catch (error) {
      const crashed = read(instance, instance.exports.output_len()) || String(error);
      instance = undefined;
      if ("crashed" in request) throw error;
      return answer({ ...request, crashed });
    }
  };
  return answer;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function ask(instance, request) {
  const { memory, buffer, run } = instance.exports;
  const encoded = encoder.encode(JSON.stringify(request));
  const at = buffer(encoded.length);
  new Uint8Array(memory.buffer, at, encoded.length).set(encoded);
  return JSON.parse(read(instance, run()));
}

function read({ exports: { memory, output } }, length) {
  return decoder.decode(new Uint8Array(memory.buffer, output(), length));
}
