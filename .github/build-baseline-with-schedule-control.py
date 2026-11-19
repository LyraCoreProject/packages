#!/usr/bin/env python3
"""Add test-only schedule control to historical Modules built for SpacetimeDB 2.14."""

import hashlib
from pathlib import Path
import subprocess
import sys


def main() -> int:
    if len(sys.argv) < 3:
        raise SystemExit("usage: build-baseline-with-schedule-control.py CORE COMMAND [ARGS...]")
    core = Path(sys.argv[1]).resolve(strict=True)
    source = core / "module/src/debug/mod.rs"
    original = source.read_bytes()
    fixture = Path(__file__).parent / "fixtures/baseline-schedule.rs"
    overlay = fixture.read_bytes()
    if b"fn debug_clear_schedule(" in original:
        raise SystemExit("baseline already provides schedule control")
    print(f"Baseline schedule fixture SHA-256: {hashlib.sha256(overlay).hexdigest()}", flush=True)
    try:
        source.write_bytes(original + b"\n" + overlay)
        return subprocess.run(sys.argv[2:], cwd=core).returncode
    finally:
        source.write_bytes(original)


if __name__ == "__main__":
    raise SystemExit(main())
