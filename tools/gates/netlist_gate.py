#!/usr/bin/env python3
"""RTL netlist lowering gate (`svlang check --netlist`) across `testdata/netlist/`.

Verifies:
1. Every valid RTL testcase lowers cleanly (`exit 0`, `errors 0`, non-empty `netlist <top>`),
   and emits byte-for-byte deterministic output across repeated invocations.
2. CLI flags (`-G name=value`, `--blast-memories <N>`, `--allow-four-state-ops`,
   `--skip-initial-checks`, `--isunknown-is-false`) work end-to-end.
3. Every fail-closed testcase (`overlap_drivers.sv`, `clock_typo.sv`, `reset_typo.sv`,
   `comb_loop.sv`, and default `isunknown.sv`) exits non-zero (`exit 1`) with the expected
   diagnostic message and never silently emits a broken netlist.

    cargo build --release -p svlang-cli
    python3 tools/gates/netlist_gate.py
"""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
NETLIST_DIR = os.path.join(REPO, "testdata", "netlist")


def find_svlang() -> str:
    env_bin = os.environ.get("SVLANG") or os.environ.get("SVLANG_BIN")
    if env_bin and os.path.isfile(env_bin):
        return env_bin
    for profile in ("release", "debug"):
        cand = os.path.join(REPO, "target", profile, "svlang")
        if os.path.isfile(cand):
            return cand
    subprocess.run(["cargo", "build", "-p", "svlang-cli"], cwd=REPO, check=True)
    return os.path.join(REPO, "target", "debug", "svlang")


EXPECTED_FAIL = {
    "clock_typo.sv": "undeclared",
    "comb_loop.sv": "combinational loop",
    "comb_loop_bits.sv": "combinational loop",
    "initial_assert_false.sv": "immediate assertion in initial block is false",
    "initial_signal_check.sv": "immediate assertion on a non-constant value inside initial block",
    "isunknown.sv": "$isunknown",
    "net_two_drivers.sv": "has more than one driver",
    "overlap_drivers.sv": "overlapping drivers",
    "reset_typo.sv": "undeclared",
    "reversed_select.sv": "part-select [0:3] is reversed",
    "unknown_module.sv": "unknown module 'not_defined'",
    "unknown_param.sv": "parameter 'Depth' does not exist in 'unknown_param_leaf'",
    "unknown_port.sv": "port 'enable' does not exist in 'unknown_port_leaf'",
    "var_mixed_assigns.sv": "cannot mix continuous and procedural assignments to variable 'v'",
    "var_two_assigns.sv": "cannot have multiple continuous assignments to variable 'v'",
}


def run_svlang(svlang_bin: str, extra_args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [svlang_bin, "check", "--netlist", *extra_args],
        cwd=REPO,
        capture_output=True,
        text=True,
        errors="replace",
        timeout=60,
    )


def main() -> int:
    svlang_bin = find_svlang()
    sv_files = sorted(f for f in os.listdir(NETLIST_DIR) if f.endswith(".sv"))
    if not sv_files:
        print("error: no .sv files found in testdata/netlist", file=sys.stderr)
        return 2

    passed = 0
    failures: list[str] = []

    for fname in sv_files:
        fpath = os.path.join(NETLIST_DIR, fname)
        if fname in EXPECTED_FAIL:
            needle = EXPECTED_FAIL[fname]
            r = run_svlang(svlang_bin, [fpath])
            if r.returncode != 1:
                failures.append(f"{fname}: expected exit 1, got {r.returncode}\n{r.stdout}{r.stderr}")
                continue
            if "\nnetlist " in ("\n" + r.stdout):
                failures.append(f"{fname}: fail-closed testcase unexpectedly emitted a netlist")
                continue
            if needle not in r.stdout and needle not in r.stderr:
                failures.append(f"{fname}: missing expected diagnostic {needle!r} in:\n{r.stdout}{r.stderr}")
                continue
            passed += 1
        else:
            r1 = run_svlang(svlang_bin, [fpath])
            r2 = run_svlang(svlang_bin, [fpath])
            if r1.returncode != 0:
                failures.append(f"{fname}: expected exit 0, got {r1.returncode}\n{r1.stdout}{r1.stderr}")
                continue
            if not r1.stdout.startswith("netlist ") or "errors 0" not in r1.stdout:
                failures.append(f"{fname}: malformed netlist dump output:\n{r1.stdout}")
                continue
            if r1.stdout != r2.stdout:
                failures.append(f"{fname}: non-deterministic netlist dump across consecutive runs")
                continue
            passed += 1

    # Additional CLI flag gates:
    # 1. `-G W=6` parameter override on `counter_ok.sv`
    rc = run_svlang(svlang_bin, ["-G", "W=6", os.path.join(NETLIST_DIR, "counter_ok.sv")])
    if rc.returncode != 0 or "output cnt : u6.4" not in rc.stdout:
        failures.append(f"counter_ok.sv (-G W=6): expected u6.4 output, got rc={rc.returncode}:\n{rc.stdout}")
    else:
        passed += 1

    # 2. `--blast-memories 16` on `fifo.sv`
    rb = run_svlang(svlang_bin, ["--blast-memories", "16", os.path.join(NETLIST_DIR, "fifo.sv")])
    if rb.returncode != 0 or "\nmemory " in rb.stdout or "sync_fifo.mem[0]" not in rb.stdout:
        failures.append(f"fifo.sv (--blast-memories 16): expected blasted mem[0..3] states, got rc={rb.returncode}:\n{rb.stdout}")
    else:
        passed += 1

    # 3. `--allow-four-state-ops` on `isunknown.sv`
    ri = run_svlang(svlang_bin, ["--allow-four-state-ops", os.path.join(NETLIST_DIR, "isunknown.sv")])
    if ri.returncode != 0 or "isunknown(" not in ri.stdout:
        failures.append(f"isunknown.sv (--allow-four-state-ops): expected isunknown op, got rc={ri.returncode}:\n{ri.stdout}")
    else:
        passed += 1

    # 4. `--skip-initial-checks` on `initial_signal_check.sv`
    rs = run_svlang(svlang_bin, ["--skip-initial-checks", os.path.join(NETLIST_DIR, "initial_signal_check.sv")])
    if rs.returncode != 0 or "state s0 initial_signal_check.q" not in rs.stdout:
        failures.append(f"initial_signal_check.sv (--skip-initial-checks): expected a netlist, got rc={rs.returncode}:\n{rs.stdout}")
    else:
        passed += 1

    # 5. `--isunknown-is-false` on `isunknown.sv`
    rf = run_svlang(svlang_bin, ["--isunknown-is-false", os.path.join(NETLIST_DIR, "isunknown.sv")])
    if rf.returncode != 0 or "isunknown(" in rf.stdout or not rf.stdout.startswith("netlist "):
        failures.append(f"isunknown.sv (--isunknown-is-false): expected a netlist without isunknown, got rc={rf.returncode}:\n{rf.stdout}")
    else:
        passed += 1

    # 6. `--tristate-checks` on `inout_bus.sv`
    rt = run_svlang(svlang_bin, ["--tristate-checks", os.path.join(NETLIST_DIR, "inout_bus.sv")])
    if rt.returncode != 0 or "inout_bus.pad_io._tri_contention" not in rt.stdout:
        failures.append(f"inout_bus.sv (--tristate-checks): expected _tri_contention check, got rc={rt.returncode}:\n{rt.stdout}")
    else:
        passed += 1

    for msg in failures:
        print(f"FAIL: {msg}")
    print(f"netlist_gate: passed={passed} failed={len(failures)}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
