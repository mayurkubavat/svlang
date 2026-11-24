# Slang (third-party, optional dev-only oracles)

[Slang](https://github.com/MikePopoloski/slang) is an upstream C++ SystemVerilog
compiler front end used only by the optional developer-only differential
compiler dumpers in `tools/oracles/`. It is not part of `svlang` and is kept
out of this repository:

- **Pinned, fetched, never edited in place.** [`slang.cmake`](slang.cmake)
  holds the pin (`SVLANG_LINT_SLANG_TAG`) and fetches that commit at configure
  time into `<build>/_deps/slang-src`.
- **Patches are a last resort.** Any patch under [`patches/`](patches/) is
  applied automatically after the download. Today there are none.

## Optional Slang compiler oracles (`tools/oracles/`)

The entire `svlang` toolchain (`sv-*`, `svlang`, `svlang-model`, `svlang-lint`,
`svlang-lsp`, `svlang-cli`, `svlang-py`, `svlang-wasm`) is 100% pure Rust and
never builds, links, or depends on Slang.

Upstream Slang is fetched only when a developer explicitly builds the optional
differential compiler dumpers in `tools/oracles/` (~880 LOC across 8 standalone
`.cpp` files):

```sh
cmake --preset oracles && cmake --build build -j 4
```

Those binaries (`svlang_pp_dump`, `svlang_cst_outline`, `svlang_cst_exprs`,
`svlang_sym_dump`, `svlang_compile_check`, `svlang_exprdump`, `svlang_specdump`,
`svlang_regexcheck`) power the differential compiler conformance gates in
`tools/gates/` (`pp_parity.py`, `cst_parity.py`, `sym_gate.py`, `ieee_gate.py`,
`slang_unit_gate.py`, `expr_gate.py`, `spec_gate.py`, `regex_gate.py`). Nothing
in CI, releases, or `cargo build`/`cargo test` builds or requires them.

## Updating Slang

1. Set `SVLANG_LINT_SLANG_TAG` in [`slang.cmake`](slang.cmake) to the target Slang
   release tag (or commit hash with date and rationale).
2. Reconfigure and rebuild the oracle binaries from a clean `<build>/_deps/slang-*`:

   ```sh
   rm -rf build/_deps/slang-*
   cmake --preset oracles && cmake --build build -j 4
   ```

3. Run the differential compiler gates against the updated Slang build:

   ```sh
   tools/gates/prepush.sh --oracles
   UVM_SRC=references/corpus/uvm-2020.3.1/src python3 tools/gates/ieee_gate.py <sv-tests-dir>
   UVM_SRC=references/corpus/uvm-2020.3.1/src python3 tools/gates/sym_gate.py --types --values <sv-tests-dir>
   python3 tools/gates/expr_gate.py --repo references/corpus/uvm-2020.3.1/src
   ```

   Inspect any new discrepancies: fix genuine gaps in `sv-semantic` / `sv-syntax` /
   `sv-preprocessor` (and distill minimal `.sv` regression tests under `testdata/`),
   or report upstream Slang regressions.
4. Commit the `slang.cmake` pin update (and any `patches/` cleanup) as a single unit.

## Adding a patch

1. Open (or find) the upstream Slang issue or pull request first.
2. Make the change in a Slang checkout at the pinned commit and export it:

   ```sh
   git -C <slang> diff > third_party/slang/patches/NNNN-<issue>-<short-name>.patch
   ```

   `NNNN` is the order they apply in; `<issue>` is the upstream number
   (for example `0001-2066-crash-in-randomize-with.patch`). Start the file with
   a few lines saying what it fixes and the upstream link; `git apply` ignores
   text before the first `diff --git`.
3. Reconfigure from a clean `<build>/_deps/slang-*` so the patch step runs.
   A patch that no longer applies stops the configure with its name.

Delete a patch as soon as the pinned commit contains the fix.

`FETCHCONTENT_SOURCE_DIR_SLANG=<checkout>` builds against a local Slang
checkout. CMake skips its patch step then, and warns if there are patches:
apply them to that checkout yourself.
