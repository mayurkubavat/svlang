#!/bin/sh
# Checks a release archive the way a user meets it: unpacked somewhere else,
# with nothing of the repository beside it.
#
#     tools/release/smoke.sh svlang-TARGET.tar.gz
#
# Runs every core subcommand of the unified `svlang` binary (`lint` text/JSON/SARIF
# + Python project config discovery, `check`, `filelist`, `map`, `lsp --version`).
set -eu
[ $# -eq 1 ] || { echo "usage: $0 ARCHIVE" >&2; exit 2; }
archive=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tar -C "$work" -xzf "$archive"
root=$(echo "$work"/svlang-*)
bin=$root/bin
ext=
if [ -f "$bin/svlang.exe" ]; then
    ext=.exe
fi
svlang="$bin/svlang$ext"
cd "$work"

cat > tb.sv <<'SV'
class smoke_item;
  rand int unsigned addr;
  function void roll();
    void'(this.randomize());
    if (addr == addr) $display("same");
  endfunction
endclass
SV

"$svlang" --version | grep -q '^svlang ' || { echo "smoke: svlang --version" >&2; exit 1; }
"$svlang" lint --list | grep -q '^SVL-COR-004 ' || { echo "smoke: lint --list has no SVL-COR-004" >&2; exit 1; }

status=0
"$svlang" lint --no-config --no-cache tb.sv --json out.json --sarif out.sarif > out.txt || status=$?
[ "$status" -eq 1 ] || { echo "smoke: expected exit 1 (an ERROR finding), got $status" >&2; cat out.txt >&2; exit 1; }
grep -q 'SVL-COR-004' out.txt || { echo "smoke: SVL-COR-004 not reported" >&2; cat out.txt >&2; exit 1; }
grep -q '"ruleId": "SVL-COR-004"' out.json || { echo "smoke: JSON report lacks the finding" >&2; exit 1; }
grep -q '"version": "2.1.0"' out.sarif || { echo "smoke: SARIF report is not 2.1.0" >&2; exit 1; }

# A project config in Python: needs python3 and the SDK from the archive.
if python3 -c '' >/dev/null 2>&1; then
    mkdir -p proj/.svlang-lint
    cp tb.sv proj/tb.sv
    cat > proj/.svlang-lint/config.py <<'PY'
from svlang_lint.config import ProjectConfig, RuleConfig


class Project(ProjectConfig):
    sources = ["tb.sv"]

    class SelfComparison(RuleConfig):
        enabled = False
PY
    status=0
    (cd proj && "$svlang" lint --no-cache > ../proj.txt 2> ../proj.err) || status=$?
    if [ "$status" -ge 2 ] || grep -q 'SVL-COR-004' proj.txt; then
        echo "smoke: the Python project config was not applied (exit $status)" >&2
        cat proj.txt proj.err >&2
        exit 1
    fi
fi

"$svlang" check tb.sv > /dev/null
"$svlang" filelist tb.sv > /dev/null
"$svlang" map tb.sv > /dev/null
"$svlang" lsp --version | grep -q '^svlang lsp ' || { echo "smoke: svlang lsp --version" >&2; exit 1; }
echo "smoke: $(basename "$archive") ok"
