#!/usr/bin/env bash
# Builds svlang.wasm next to the page and runs the Node check.
#   crates/svlang-wasm/www/build.sh        then serve this directory, e.g.
#   python3 -m http.server -d crates/svlang-wasm/www 8000
# Needs the wasm32 target once: rustup target add wasm32-unknown-unknown
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/../../.."
# The front end recurses on the design's structure; the default 1 MB stack is small for it.
RUSTFLAGS="-C link-arg=-zstack-size=67108864" \
    cargo build --locked --profile dist --target wasm32-unknown-unknown -p svlang-wasm "$@"
cp target/wasm32-unknown-unknown/dist/svlang_wasm.wasm "$here/svlang.wasm"
ls -l "$here/svlang.wasm"
if command -v node >/dev/null 2>&1; then
    node "$here/test.mjs"
else
    echo "node not found: skipped www/test.mjs"
fi
