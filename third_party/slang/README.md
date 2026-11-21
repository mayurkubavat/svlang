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

1. Run the canary against the candidate (master by default). It builds a
   separate tree, applies our patches, and runs the suites that measure Slang:

   ```sh
   tools/quality/slang_canary.py --work ../slang-canary [--ref <tag-or-commit>] \
       [--uvm-home ../uvm-core] --report canary.md
   ```

2. Read `canary.md`:
   - build failed: Slang's API changed; fix the C++ code that uses it;
   - a patch is "already upstream": delete it from `patches/`;
   - a patch "does not apply": rebase it on the new commit (below) or drop it;
   - `slang_overlay_test` fails naming an override: Slang fixed that bug;
     delete the override (or retire the crash pattern);
   - rule suites changed: decide whether each change is a Slang fix or a
     regression; report regressions upstream.
3. Move the pin: set `SVLANG_LINT_SLANG_TAG` in `slang.cmake` (a release tag when
   one has what we need, else a master commit, with its date and why).
4. Rebuild from clean (`rm -rf build build-model`, or at least
   `<build>/_deps/slang-*`), then refresh what depends on Slang's output and
   review the diff:

   ```sh
   cmake --preset reference && cmake --build build && ctest --test-dir build
   python3 cpp/tests/regression.py --mode all-rules --update-baseline \
       --baseline cpp/tests/baselines/baseline_stub.json
   cmake --preset model && cmake --build build-model && ctest --test-dir build-model
   ```

   Rerun the parity gates in `tools/gates/` and the corpus gate, since they
   compare the native front end with Slang.
5. Commit the pin, the patch changes, the refreshed baselines and the canary
   report in one commit, so a bump can be reverted as a unit.

## Adding a patch

1. Check that an override in `cpp/src/compat/` cannot do it.
2. Open (or find) the upstream issue or pull request first.
3. Make the change in a Slang checkout at the pinned commit and export it:

   ```sh
   git -C <slang> diff > third_party/slang/patches/NNNN-<issue>-<short-name>.patch
   ```

   `NNNN` is the order they apply in; `<issue>` is the upstream number
   (for example `0001-2066-crash-in-randomize-with.patch`). Start the file with
   a few lines saying what it fixes and the upstream link; `git apply` ignores
   text before the first `diff --git`.
4. Reconfigure from a clean `<build>/_deps/slang-*` so the patch step runs.
   A patch that no longer applies stops the configure with its name.

Delete a patch as soon as the pinned commit contains the fix; the canary says
when ("already upstream").

`FETCHCONTENT_SOURCE_DIR_SLANG=<checkout>` builds against a local Slang
checkout (as the canary does). CMake skips its patch step then, and warns if
there are patches: apply them to that checkout yourself.
