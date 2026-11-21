#!/usr/bin/env bash
# Everything CI runs on a push, run locally first: rust_gate (fmt/clippy),
# cargo test, netlist gate, rule docs freshness, end-to-end Python integration
# suites, release archive + smoke test, and the uvm-core ERROR budget.
#
#   tools/gates/prepush.sh              # full Rust + integration flow
#   tools/gates/prepush.sh --quick      # skip slow dist archive and uvm-core steps
#   tools/gates/prepush.sh --oracles    # also run Slang differential gates (needs build/ oracles)
set -u
DVQ="${DVQ-$HOME/.local/bin/dvq run build --}"; [ -x "$HOME/.local/bin/dvq" ] || DVQ=
if [ -n "$DVQ" ] && [ -z "${DVQ_CLASS:-}${RDEV_JOB_ID:-}" ]; then
    export DVQ_CPU="${DVQ_CPU:-${PREPUSH_JOBS:-4}00%}" DVQ_MEM="${DVQ_MEM:-10G}"
    exec $DVQ "$(cd "$(dirname "$0")" && pwd)/$(basename "$0")" "$@"
fi
cd "$(dirname "$0")/../.."

quick=0 oracles=0
for a in "$@"; do
    case "$a" in
        --rust) ;;
        --quick) quick=1 ;;
        --oracles) oracles=1 ;;
        *) echo "usage: tools/gates/prepush.sh [--quick] [--oracles]" >&2; exit 2 ;;
    esac
done

log=$(mktemp -d)
failed=() skipped=()

step() {
    local name=$1; shift
    local out="$log/$(echo "$name" | tr -c 'A-Za-z0-9' '_').log"
    local t0=$SECONDS
    if "$@" >"$out" 2>&1; then
        printf '  ok    %-52s %4ss\n' "$name" $((SECONDS - t0))
    else
        printf '  FAIL  %-52s %4ss   (%s)\n' "$name" $((SECONDS - t0)) "$out"
        tail -n 15 "$out" | sed 's/^/        /'
        failed+=("$name")
    fi
}
skip() {
    printf '  ----  %-52s %s\n' "$1" "$2"
    skipped+=("$1")
}
uvm_ok() { [ -n "${UVM_HOME:-}" ] && [ -f "$UVM_HOME/src/uvm_pkg.sv" ]; }

uvm_ref=2020.3.1
if [ -z "${UVM_HOME:-}" ] && [ "$quick" = 0 ]; then
    UVM_HOME="${XDG_CACHE_HOME:-$HOME/.cache}/svlang/uvm-core-$uvm_ref"
    export UVM_HOME
    if ! uvm_ok; then
        echo "fetching uvm-core $uvm_ref into $UVM_HOME (once per machine)"
        mkdir -p "$(dirname "$UVM_HOME")"
        git -c advice.detachedHead=false clone -q --depth 1 --branch "$uvm_ref" \
            https://github.com/accellera-official/uvm-core.git "$UVM_HOME.tmp.$$" \
            && mv "$UVM_HOME.tmp.$$" "$UVM_HOME" || rm -rf "$UVM_HOME.tmp.$$"
    fi
fi

step "hygiene: no assistant state, keys or attribution" tools/gates/hygiene.sh

echo "Rust & integration flow (rust.yml)"
step "rust_gate: guidelines, fmt, clippy" tools/gates/rust_gate.sh
step "cargo test" cargo test --locked
step "netlist gate (svlang check --netlist)" sh -c \
    'cargo build --locked -q -p svlang-cli && SVLANG=target/debug/svlang python3 tools/gates/netlist_gate.py'
step "rule docs are current" sh -c \
    'cargo build --locked -q -p svlang-cli && python3 tools/codegen/gen_rule_docs.py --binary target/debug/svlang --check'
step "build svlang (release)" cargo build --locked --release -p svlang-cli --bin svlang

b=target/release/svlang
for t in baseline catalog changed explain fix_invariants guidelines history init map precision waivers; do
    step "tests/test_$t" python3 "tests/test_$t.py" --binary "$b"
done
step "python/tests/test_python_rules" python3 python/tests/test_python_rules.py --binary "$b"

if [ "$quick" = 1 ]; then
    skip "release archive and smoke test" "skipped by --quick"
    skip "ERROR budget (uvm-core)" "skipped by --quick"
else
    step "dist build" cargo build --locked --profile dist -p svlang-cli
    plat="$(uname -s | tr 'A-Z' 'a-z')-$(uname -m | sed 's/x86_64/x64/; s/aarch64/arm64/')"
    step "release archive and smoke test" sh -c \
        "rm -rf '$log/dist' && tools/release/package.sh target/dist '$plat' '$log/dist' && tools/release/smoke.sh '$log/dist/svlang-$plat.tar.gz'"
    if uvm_ok; then
        step "ERROR budget (uvm-core)" python3 tools/quality/error_budget.py --binary "$b"
    else
        skip "ERROR budget (uvm-core)" "NOT RUN: no uvm-core 2020.3.1 checkout"
        failed+=("ERROR budget (uvm-core): no uvm-core checkout")
    fi
fi

if [ "$oracles" = 1 ]; then
    echo "Optional Slang differential oracle gates"
    [ -f build/build.ninja ] || step "configure oracles" cmake --preset oracles
    step "build oracles" cmake --build build -j 4
    step "slang unit gate (svlang check vs slang)" python3 tools/gates/slang_unit_gate.py --max-false=1 --max-gaps=370
    if uvm_ok; then
        step "spec gate (--model, uvm-core)" python3 tools/gates/spec_gate.py --model --max-differ=0 --uvm "$UVM_HOME/src"
    fi
fi

echo
if [ ${#failed[@]} -gt 0 ]; then
    echo "prepush: ${#failed[@]} FAILED, do not push"
    printf '  %s\n' "${failed[@]}"
    echo "logs: $log"
    exit 1
fi
if [ ${#skipped[@]} -gt 0 ]; then
    echo "prepush: green, but ${#skipped[@]} step(s) skipped by --quick: run without it before the push"
    exit 0
fi
echo "prepush: all green"
