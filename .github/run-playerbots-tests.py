#!/usr/bin/env python3
"""Run Package-owned tests against an explicitly selected Core checkout."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib


def main() -> int:
    if len(sys.argv) < 3:
        raise SystemExit("usage: run-playerbots-tests.py CORE COMMAND [CARGO-ARGS...]")
    core = Path(sys.argv[1]).resolve(strict=True)
    command, *arguments = sys.argv[2:]
    collection = Path(__file__).resolve().parent.parent
    suite = collection / "playerbots/testsuite"
    installed = core / "packages/playerbots"
    if not installed.is_dir() or installed.resolve() != collection / "playerbots":
        raise SystemExit("run through check-core-tip.sh so the tested Package is installed")
    toolchain = tomllib.loads((core / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    environment = os.environ.copy()
    environment["LYRACORE_TEST_CORE"] = str(core)
    environment.setdefault("CARGO_TARGET_DIR", str(core / "target"))
    cargo = ["cargo", f"+{toolchain}"]
    if command in {"test", "check", "clippy", "fmt", "generate-lockfile"}:
        patches = []
        for name in ["lyracore-shared", "lyracore-test-support"]:
            path = core / "crates" / name
            if not (path / "Cargo.toml").is_file():
                raise SystemExit(f"Core does not provide {name}: {path}")
            patches += ["--config", f"patch.crates-io.{name}.path={json.dumps(str(path))}"]
        # cargo-clippy forwards only the options after its subcommand to Cargo.
        cargo += ([*patches, command] if command == "fmt" else [command, *patches])
        cargo += ["--manifest-path", str(suite / "Cargo.toml"), *arguments]
    else:
        raise SystemExit(f"unsupported Cargo command: {command}")
    return subprocess.run(cargo, cwd=core, env=environment).returncode


if __name__ == "__main__":
    raise SystemExit(main())
