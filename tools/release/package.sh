#!/bin/sh
# Packages the unified `svlang` native binary as the release ships it.
#
#     tools/release/package.sh BIN_DIR TARGET OUT_DIR
#
# BIN_DIR holds the built binary (target/<triple>/dist), TARGET names the
# platform (linux-x64, darwin-arm64, ...). Writes
# OUT_DIR/svlang-TARGET.tar.gz:
#
#     svlang-TARGET/bin/      svlang (or svlang.exe on Windows)
#     svlang-TARGET/python/   the Python SDK (config.py, Python rules);
#                             `svlang lint` finds it beside bin/
#     svlang-TARGET/LICENSE, NOTICE, README.md
set -eu
[ $# -eq 3 ] || { echo "usage: $0 BIN_DIR TARGET OUT_DIR" >&2; exit 2; }
bin=$1 target=$2 out=$3
repo=$(cd "$(dirname "$0")/../.." && pwd)
name=svlang-$target
stage=$out/$name
rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/python"
ext=
case "$target" in
    win32-*) ext=.exe ;;
esac
cp "$bin/svlang$ext" "$stage/bin/svlang$ext"
[ -n "$ext" ] || chmod 755 "$stage/bin/svlang"
cp -R "$repo/python/svlang_lint" "$stage/python/svlang_lint"
find "$stage/python" -name __pycache__ -type d -prune -exec rm -rf {} +
cp "$repo/LICENSE" "$repo/NOTICE" "$stage/"
cp "$repo/tools/release/README.md" "$stage/README.md"
tar -C "$out" -czf "$out/$name.tar.gz" "$name"
rm -rf "$stage"
echo "$out/$name.tar.gz"
