# svlang in the browser

[svlang](https://github.com/mayurkubavat/svlang) compiled to WebAssembly, and
a playground page built on it: type SystemVerilog on the left, see the
diagnostics, syntax tree, elaborated hierarchy, netlist, Btor2, AIGER or
Yosys JSON on the right. Everything runs in the page; nothing is uploaded.

The module is about 2.5 MB (0.8 MB compressed) and needs no JavaScript
libraries and no generated bindings.

## Build and run

```sh
rustup target add wasm32-unknown-unknown      # once
crates/svlang-wasm/www/build.sh               # builds www/svlang.wasm, runs the Node check
python3 -m http.server -d crates/svlang-wasm/www 8000
```

Then open <http://localhost:8000>. The page is three static files
(`index.html`, `svlang.js`, `svlang.wasm`), so any static host serves it.

## Use it from your own page or from Node

```js
import { load } from "./svlang.js";

const run = await load("./svlang.wasm");          // in Node: load(readFileSync("svlang.wasm"))
const reply = run("check", "module m; endmodule\n");
// { ok, output, diagnostics: [{ line, column, severity, stage, message }], notes }
```

The commands are `check`, `preprocess`, `cst`, `hier`, `netlist`, `btor2`,
`aiger` and `yosys`; an optional third argument names the top module. The
source is one text: `` `include `` has no files to read here. For the
writers, memories of up to 1,024 words are expanded into registers and the
concurrent properties are compiled into checks.

From another language, call the module's C interface directly:
`svlang_alloc(len)`, `svlang_free(ptr, len)` and
`svlang_run(command, len, source, len, top, len)`, which returns a pointer to
a 4-byte little-endian length followed by that many bytes of JSON.

## License

Apache-2.0.
