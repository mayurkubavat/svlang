#!/bin/sh
# Format and lint gate for the pure-Rust workspace: generated guidelines check,
# cargo fmt --check, Clippy with -D warnings on all targets, and optional MSRV
# compile check when rustup has the workspace MSRV toolchain installed.
#
#     tools/gates/rust_gate.sh [--native]
set -eu
cd "$(dirname "$0")/../.."

case "${1-}" in
    ""|--native) ;;
    *) echo "usage: $0 [--native]" >&2; exit 2 ;;
esac

step() { echo "== $*"; "$@"; }

step python3 tools/codegen/gen_guidelines.py --check
step cargo fmt --all --check
step cargo clippy --all-targets --locked -- -D warnings

msrv=$(sed -n 's/^rust-version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)
want=${SVLANG_LINT_MSRV:-auto}
if [ "$want" != no ] && [ -n "$msrv" ] && command -v rustup >/dev/null 2>&1 \
    && rustup run "$msrv" rustc --version >/dev/null 2>&1; then
    # svlang-py (the Python wheel) follows PyO3's newer floor; see its Cargo.toml.
    step rustup run "$msrv" cargo check --workspace --exclude svlang-py --all-targets --locked --target-dir target/msrv
elif [ "$want" = yes ]; then
    echo "rust_gate: SVLANG_LINT_MSRV=yes needs rustup with toolchain $msrv" >&2
    exit 2
else
    echo "== skipped the MSRV $msrv leg (no rustup toolchain $msrv)"
fi
echo "rust_gate: clean"
