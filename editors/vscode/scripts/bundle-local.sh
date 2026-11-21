#!/bin/sh
# Copies the locally built `svlang` binary into bin/<platform>-<arch>/ so
# `npm run package` makes a self-contained .vsix for this machine. Build first:
#   cargo build --release -p svlang-cli
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
src="${SVLANG_BIN:-$repo/target/release/svlang$ext}"
if [ -f "$src" ]; then
    cp "$src" "$dest/svlang$ext"
    [ -n "$ext" ] || chmod +x "$dest/svlang"
    echo "bundled svlang$ext -> bin/$plat/"
else
    echo "not found: $src (run: cargo build --release -p svlang-cli)" >&2
    exit 1
fi
