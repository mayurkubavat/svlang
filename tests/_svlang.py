"""Shared CLI invocation helpers for end-to-end `svlang lint` integration tests."""

from pathlib import Path


REPO = Path(__file__).resolve().parents[1]


def default_binary() -> str:
    for profile in ("release", "dist", "debug"):
        cand = REPO / "target" / profile / "svlang"
        if cand.is_file():
            return str(cand)
    return str(REPO / "target" / "release" / "svlang")


def lint_cmd(binary: str) -> list[str]:
    p = Path(binary)
    return [binary, "lint"] if p.stem == "svlang" else [binary]
