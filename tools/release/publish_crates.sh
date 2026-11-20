#!/usr/bin/env bash
# Publish the 11 publishable svlang crates to crates.io in strict topological
# dependency order:
#
#   tools/release/publish_crates.sh --dry-run    # verify metadata + package tarballs locally
#   tools/release/publish_crates.sh --execute    # publish all 11 crates to crates.io
#
# Tier order (each tier depends only on earlier tiers):
#   1. sv-source, sv-lexer
#   2. sv-preprocessor
#   3. sv-syntax
#   4. sv-semantic
#   5. sv-netlist
#   6. sv-btor2, sv-yosys
#   7. sv-aiger
#   8. svlang
#   9. svlang-cli
set -euo pipefail

cd "$(dirname "$0")/../.."

mode="${1:---dry-run}"
case "$mode" in
    --dry-run | --execute) ;;
    *)
        echo "usage: $0 [--dry-run | --execute]" >&2
        exit 2
        ;;
esac

CRATES=(
    sv-source
    sv-lexer
    sv-preprocessor
    sv-syntax
    sv-semantic
    sv-netlist
    sv-btor2
    sv-yosys
    sv-aiger
    svlang
    svlang-cli
)

ver=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)
echo "svlang crates.io release ($mode) for version $ver"

if [ "$mode" = "--dry-run" ]; then
    # Root leaf crates have no unpublished workspace dependencies, so `cargo package --locked`
    # validates full tarball verification including compile check; dependent crates validate
    # manifest metadata and file inclusion with `--no-verify` before upstream tiers are on crates.io.
    for c in sv-source sv-lexer; do
        echo "== packaging + verifying leaf crate $c ($ver)"
        CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" cargo package --locked --allow-dirty -p "$c"
    done
    for c in "${CRATES[@]:2}"; do
        echo "== verifying manifest & file list for $c ($ver)"
        files=$(cargo package --list --allow-dirty -p "$c" | wc -l | tr -d ' ')
        echo "   $c: $files packaged files"
    done
    python3 -c '
import json, subprocess, sys
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
want = {"sv-source","sv-lexer","sv-preprocessor","sv-syntax","sv-semantic","sv-netlist","sv-btor2","sv-yosys","sv-aiger","svlang","svlang-cli"}
by_name = {p["name"]: p for p in meta["packages"]}
missing = []
for name in sorted(want):
    p = by_name[name]
    for field in ("description", "license", "repository", "readme"):
        if not p.get(field):
            missing.append(f"{name} missing {field}")
if missing:
    sys.exit("FAIL: " + "; ".join(missing))
print(f"cargo metadata: all {len(want)} publishable crates have description, license, repository, and readme")
'
    echo "publish_crates.sh: dry-run clean (${#CRATES[@]} crates packaged)"
    exit 0
fi

for c in "${CRATES[@]}"; do
    echo "== publishing $c@$ver to crates.io"
    attempt=1
    while true; do
        out_file=$(mktemp)
        if CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" cargo publish --locked -p "$c" >"$out_file" 2>&1; then
            cat "$out_file"
            rm -f "$out_file"
            break
        fi
        cat "$out_file" >&2
        if grep -q "already uploaded\|already exists" "$out_file"; then
            echo "   $c@$ver is already published; continuing"
            rm -f "$out_file"
            break
        fi
        if [ "$attempt" -lt 4 ] && grep -q "failed to select a version\|no matching package named" "$out_file"; then
            rm -f "$out_file"
            echo "   waiting for crates.io sparse-index propagation (attempt $attempt/4)..."
            python3 -c "import time; time.sleep(6)"
            attempt=$((attempt + 1))
            continue
        fi
        rm -f "$out_file"
        exit 1
    done
done

echo "publish_crates.sh: published all ${#CRATES[@]} crates at $ver"
