#!/bin/sh
# Packages the native tools as the release ships them.
#
#     tools/release/package.sh BIN_DIR TARGET OUT_DIR
#
# BIN_DIR holds the built binaries (target/<triple>/dist), TARGET names the
# platform (linux-x64, darwin-arm64, ...). Writes
# OUT_DIR/svlang-lint-TARGET.tar.gz:
#
#     svlang-lint-TARGET/bin/      svlang-lint, dv-lsp, dv-build, dv-map, dv-filelist
#     svlang-lint-TARGET/python/   the Python SDK (config.py, Python rules);
#                               svlang-lint finds it beside bin/
#     svlang-lint-TARGET/LICENSE, NOTICE, README.md
set -eu
[ $# -eq 3 ] || { echo "usage: $0 BIN_DIR TARGET OUT_DIR" >&2; exit 2; }
bin=$1 target=$2 out=$3
repo=$(cd "$(dirname "$0")/../.." && pwd)
name=svlang-lint-$target
stage=$out/$name
rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/python"
ext=
case "$target" in
    win32-*) ext=.exe ;;
esac
for tool in svlang-lint dv-lsp dv-build dv-map dv-filelist; do
    cp "$bin/$tool$ext" "$stage/bin/$tool$ext"
    [ -n "$ext" ] || chmod 755 "$stage/bin/$tool"
done
cp -R "$repo/python/svlang_lint" "$stage/python/svlang_lint"
find "$stage/python" -name __pycache__ -type d -prune -exec rm -rf {} +
cp "$repo/LICENSE" "$repo/NOTICE" "$stage/"
cp "$repo/tools/release/README.md" "$stage/README.md"
tar -C "$out" -czf "$out/$name.tar.gz" "$name"
rm -rf "$stage"
echo "$out/$name.tar.gz"
