#!/usr/bin/env python3
"""
Checks the rule catalog generated from the rules' own metadata:

- every rule and sub-rule has a unique ID and a category from the closed set;
- every ID a rule test expects appears in the catalog;
- `svlang lint --list-rules --json` matches docs/rules/catalog.json exactly.
"""

import argparse
import json
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

from _svlang import REPO, default_binary, lint_cmd


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", default=default_binary())
    args = ap.parse_args()
    binary = str(Path(args.binary).resolve())
    failures = []

    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "catalog.json"
        res = subprocess.run([*lint_cmd(binary), "--list-rules", "--json", str(out)], capture_output=True,
                             text=True, cwd=tmp)
        if res.returncode != 0:
            sys.exit(f"--list-rules --json failed: {res.stderr}")
        catalog = json.loads(out.read_text())

    rules = catalog["rules"]
    categories = set(catalog["categories"])
    dups = [i for i, n in Counter(r["id"] for r in rules).items() if n > 1]
    if dups:
        failures.append(f"duplicate rule IDs: {dups}")
    for r in rules:
        if r["category"] not in categories:
            failures.append(f"{r['id']}: category {r['category']!r} is not one of {sorted(categories)}")
        if not r["description"].strip():
            failures.append(f"{r['id']}: empty description")

    published = json.loads((REPO / "docs" / "rules" / "catalog.json").read_text())
    known = {r["id"] for r in rules}
    for expect in (REPO / "testdata" / "rules").glob("*/*/expect.json"):
        for findings in json.loads(expect.read_text()).values():
            for f in findings:
                if f["rule"] not in known:
                    failures.append(f"{expect.relative_to(REPO)} expects undeclared rule {f['rule']}")

    if published["categories"] != catalog["categories"]:
        failures.append("categories differ from docs/rules/catalog.json")
    pub_by_id = {r["id"]: r for r in published["rules"]}
    mine = {r["id"]: r for r in rules}
    for rid in sorted(set(pub_by_id) | set(mine)):
        if rid not in mine:
            failures.append(f"{rid}: in docs/rules/catalog.json, not registered by binary")
        elif rid not in pub_by_id:
            failures.append(f"{rid}: registered by binary, missing from docs/rules/catalog.json")
        else:
            for key in sorted(set(pub_by_id[rid]) | set(mine[rid])):
                if pub_by_id[rid].get(key) != mine[rid].get(key):
                    failures.append(f"{rid}: {key} is {mine[rid].get(key)!r} here, "
                                    f"{pub_by_id[rid].get(key)!r} in docs/rules/catalog.json")

    for f in failures:
        print(f"FAIL: {f}")
    print(f"rule catalog: {len(rules)} entries, {len(failures)} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
