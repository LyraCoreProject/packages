#!/usr/bin/env python3
"""Read dungeons/README.md#verification before running against a Core checkout."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib


def main() -> int:
    if len(sys.argv) < 3:
        raise SystemExit("usage: run.py CORE COMMAND [CARGO-ARGS...]")
    core = Path(sys.argv[1]).resolve(strict=True)
    command, *arguments = sys.argv[2:]
    suite = Path(__file__).resolve().parent
    installed = core / "packages/dungeons"
    if not installed.is_dir() or installed.resolve() != suite.parent:
        raise SystemExit("install this dungeons Package in the selected Core checkout first")
    if command not in {"test", "check", "clippy", "fmt", "generate-lockfile"}:
        raise SystemExit(f"unsupported Cargo command: {command}")
    toolchain = tomllib.loads((core / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    support = core / "crates/lyracore-test-support"
    if not (support / "Cargo.toml").is_file():
        raise SystemExit(f"Core does not provide lyracore-test-support: {support}")
    environment = os.environ.copy()
    environment["LYRACORE_TEST_CORE"] = str(core)
    environment.setdefault("CARGO_TARGET_DIR", str(core / "target"))
    patch = ["--config", f"patch.crates-io.lyracore-test-support.path={json.dumps(str(support))}"]
    cargo = ["cargo", f"+{toolchain}"]
    cargo += [*patch, command] if command == "fmt" else [command, *patch]
    cargo += ["--manifest-path", str(suite / "Cargo.toml"), *arguments]
    return subprocess.run(cargo, cwd=core, env=environment).returncode


if __name__ == "__main__":
    raise SystemExit(main())
