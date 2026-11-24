# MG-8 Contracts and failure handling

Grounded in Meyer's design by contract and the fail-fast principle.

A check that cannot fail the test is not a check. Failures go through the UVM report server at error severity, so they are counted, can be waived deliberately and appear in the summary.

## MG-8.1 (Shall)

**Every check that can fail reports through UVM at error severity, so failures are counted and never hidden.**

A mismatch reported as a warning lets the test pass; `uvm_fatal on the first mismatch hides how many there were. Unchecked randomize(), $cast() and config_db get() results fail silently.

Instead of:

```systemverilog
if (!act.compare(exp)) `uvm_warning("SB", "mismatch")
```

Write:

```systemverilog
if (!act.compare(exp)) `uvm_error("SB", "mismatch")
```

Enforced by: `SVA-PRP-008`, `SVL-CON-025`, `SVL-COR-019`, `SVL-COR-020`, `SVL-COR-021`, `SVL-OOP-002`, `SVL-RND-001`, `SVL-SEC-002`, `UVM-CFG-004`, `UVM-CMP-009`, `UVM-CMP-014`, `UVM-CMP-015`, `UVM-CMP-016`, `UVM-SEQ-006`, `UVM-TLM-002`, `UVM-TLM-010`, `UVM-TLM-011`, `UVM-TLM-012`

## MG-8.2 (Should)

**Reusable interfaces carry their own protocol assertions.**

An interface that carries its own assertions checks the protocol in every testbench that uses it, including ones written later.

Enforced by: `SVA-PRP-016`
