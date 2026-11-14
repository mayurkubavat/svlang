// Loads svlang.wasm and gives one function: run(command, source, top) -> reply object.
// Works in a browser (pass a URL or Response) and in Node (pass the file's bytes).

export async function load(wasm) {
  const source = wasm instanceof Uint8Array || wasm instanceof ArrayBuffer ? wasm : await (await fetch(wasm)).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(source, {});
  const x = instance.exports;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  // Copies a string into the module's memory; returns [pointer, length].
  function put(text) {
    const bytes = encoder.encode(text);
    const ptr = x.svlang_alloc(bytes.length);
    new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
    return [ptr, bytes.length];
  }

  // command: check, preprocess, cst, hier, netlist, btor2, aiger or yosys.
  // Returns {ok, output, diagnostics: [{line, column, severity, stage, message}], notes}.
  return function run(command, text, top = "") {
    const args = [put(command), put(text), put(top)];
    try {
      const out = x.svlang_run(...args.flat());
      const length = new DataView(x.memory.buffer).getUint32(out, true);
      const json = decoder.decode(new Uint8Array(x.memory.buffer, out + 4, length));
      x.svlang_free(out, length + 4);
      return JSON.parse(json);
    } finally {
      for (const [ptr, length] of args) x.svlang_free(ptr, length);
    }
  };
}
