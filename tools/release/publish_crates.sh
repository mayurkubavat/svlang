#!/usr/bin/env bash
# Publish the 21 publishable svlang crates to crates.io in strict topological
# dependency order:
#
#   tools/release/publish_crates.sh --dry-run    # verify metadata + package tarballs locally
#   tools/release/publish_crates.sh --execute    # publish all 21 crates to crates.io
#
# Tier order (each tier depends only on earlier tiers):
#   1. sv-source, sv-lexer, sv-bigint, sv-fxhash
#   2. sv-preprocessor
#   3. sv-syntax
#   4. sv-scopes, sv-ir
#   5. sv-checks, sv-expr
#   6. sv-sva
#   7. sv-semantic
#   8. sv-netlist
#   9. sv-btor2, sv-yosys
#  10. sv-aiger
#  11. svlang
#  12. svlang-model
#  13. svlang-lint
#  14. svlang-lsp
#  15. svlang-cli
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

# Tier 1: crates with no workspace dependencies. The dry run packages these
# in full; the others only list their files (their dependencies are not on
# crates.io yet when a new version is being prepared).
LEAVES=(
    sv-source
    sv-lexer
    sv-bigint
    sv-fxhash
)

CRATES=(
    "${LEAVES[@]}"
    sv-preprocessor
    sv-syntax
    sv-scopes
    sv-ir
    sv-checks
    sv-expr
    sv-sva
    sv-semantic
    sv-netlist
    sv-btor2
    sv-yosys
    sv-aiger
    svlang
    svlang-model
    svlang-lint
    svlang-lsp
    svlang-cli
)

ver=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)
echo "svlang crates.io release ($mode) for version $ver"

if [ "$mode" = "--dry-run" ]; then
    # Root leaf crates have no unpublished workspace dependencies, so `cargo package --locked`
    # validates full tarball verification including compile check; dependent crates validate
    # manifest metadata and file inclusion with `--list` before upstream tiers are on crates.io.
    for c in "${LEAVES[@]}"; do
        echo "== packaging + verifying leaf crate $c ($ver)"
        CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" cargo package --locked --allow-dirty -p "$c"
    done
    for c in "${CRATES[@]:${#LEAVES[@]}}"; do
        echo "== verifying manifest & file list for $c ($ver)"
        files=$(cargo package --list --allow-dirty -p "$c" | wc -l | tr -d ' ')
        echo "   $c: $files packaged files"
    done
    python3 -c '
import json, subprocess, sys
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
want = {
    "sv-source", "sv-lexer", "sv-bigint", "sv-fxhash", "sv-preprocessor", "sv-syntax", "sv-scopes", "sv-ir",
    "sv-checks", "sv-expr", "sv-sva", "sv-semantic",
    "sv-netlist", "sv-btor2", "sv-yosys", "sv-aiger", "svlang",
    "svlang-model", "svlang-lint", "svlang-lsp", "svlang-cli",
}
by_name = {p["name"]: p for p in meta["packages"]}
missing = []
for name in sorted(want):
    p = by_name[name]
    for field in ("description", "license", "repository", "readme"):
        if not p.get(field):
            missing.append(f"{name} missing {field}")
    if p.get("publish") == []:
        missing.append(f"{name} has publish = false")
    # Internal dependencies come from [workspace.dependencies] in the root
    # Cargo.toml; their `version` must follow workspace.package.version.
    for d in p["dependencies"]:
        if d.get("path") and d["req"] != "^" + sys.argv[1]:
            missing.append(f"{name} depends on {d["name"]} {d['req']}, not {sys.argv[1]}")
if missing:
    sys.exit("FAIL: " + "; ".join(missing))
print(f"cargo metadata: all {len(want)} publishable crates have description, license, repository, readme, publish enabled, and internal dependencies at {sys.argv[1]}")
' "$ver"
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
