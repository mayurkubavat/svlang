// node www/test.mjs: the built module, driven the way the page drives it.
import { readFileSync } from "node:fs";
import { strict as assert } from "node:assert";
import { load } from "./svlang.js";

const wasm = readFileSync(new URL("./svlang.wasm", import.meta.url));
const run = await load(wasm);

const counter = `module cnt (input logic clk, rst, output logic [3:0] q);
  always_ff @(posedge clk) if (rst) q <= '0; else if (q != 9) q <= q + 1'b1;
  always @(posedge clk) in_range: assert (q <= 9);
endmodule
`;

assert.deepEqual(run("check", counter), { ok: true, output: "", diagnostics: [], notes: [] });
assert.equal(run("hier", counter).output, "$root instance cnt : cnt");
assert.match(run("cst", counter).output, /^SourceFile\n  ModuleDecl/);
assert.match(run("netlist", counter).output, /^netlist cnt\n/);
assert.match(run("btor2", counter).output, / bad \d+ cnt\.in_range\n/);
assert.match(run("aiger", counter).output, /^aag /);
assert.equal(JSON.parse(run("yosys", counter).output).modules.cnt.cells["cnt.in_range"].type, "$assert");
assert.match(run("preprocess", "`define W 8\nmodule m; logic [`W-1:0] x; endmodule\n").output, /logic \[8-1:0\] x;/);

const bad = run("check", "module m;\n  assign x = ;\nendmodule\n");
assert.equal(bad.ok, false);
assert.deepEqual(
  { line: bad.diagnostics[0].line, column: bad.diagnostics[0].column, stage: bad.diagnostics[0].stage },
  { line: 2, column: 14, stage: "parser" },
);

// Non-ASCII text survives the trip, and many calls do not run out of memory.
assert.equal(run("check", "// größe ✓\nmodule m; endmodule\n").ok, true);
for (let i = 0; i < 200; i++) assert.equal(run("btor2", counter).ok, true);

// A design with every kind of check: semantic errors come with their place.
const undeclared = run("check", "module m (output logic y);\n  assign y = nope;\nendmodule\n");
assert.equal(undeclared.ok, false);
assert.match(undeclared.diagnostics[0].message, /nope/);

console.log("svlang.wasm: all checks pass");
