#!/bin/sh
# Copies the locally built server into bin/<platform>-<arch>/ so `npm run
# package` makes a self-contained .vsix for this machine. Build it first:
#   cargo build --release -p dv-lsp
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$here/../.." && pwd)
plat=$(node -p 'process.platform + "-" + process.arch')
ext=
case "$plat" in
    win32-*) ext=.exe ;;
esac
dest="$here/bin/$plat"
rm -rf "$here/bin"
mkdir -p "$dest"
src="${DV_LSP_BIN:-$repo/target/release/dv-lsp$ext}"
if [ -f "$src" ]; then
    cp "$src" "$dest/dv-lsp$ext"
    [ -n "$ext" ] || chmod +x "$dest/dv-lsp"
    echo "bundled dv-lsp$ext -> bin/$plat/"
else
    echo "not found: $src (run: cargo build --release -p dv-lsp)" >&2
    exit 1
fi
