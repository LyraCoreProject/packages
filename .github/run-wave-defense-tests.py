#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib


def main() -> int:
    if len(sys.argv) < 3:
        raise SystemExit("usage: run-wave-defense-tests.py CORE COMMAND [CARGO-ARGS...]")
    core = Path(sys.argv[1]).resolve(strict=True)
    command, *arguments = sys.argv[2:]
    collection = Path(__file__).resolve().parent.parent
    for name in ["wave-defense", "playerbots"]:
        installed = core / "packages" / name
        if not installed.is_dir() or installed.resolve() != collection / name:
            raise SystemExit(f"install the Collection's {name} Package before running its tests")
    if command not in {"test", "check", "clippy", "fmt", "generate-lockfile"}:
        raise SystemExit(f"unsupported Cargo command: {command}")
    toolchain = tomllib.loads((core / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    environment = os.environ.copy()
    environment["LYRACORE_TEST_CORE"] = str(core)
    environment.setdefault("CARGO_TARGET_DIR", str(core / "target"))
    patches = []
    for name in ["lyracore-shared", "lyracore-test-support", "lyracore-package-delta"]:
        path = core / "crates" / name
        if not (path / "Cargo.toml").is_file():
            raise SystemExit(f"Core does not provide {name}: {path}")
        patches += ["--config", f"patch.crates-io.{name}.path={json.dumps(str(path))}"]
    cargo = ["cargo", f"+{toolchain}"]
    cargo += [*patches, command] if command == "fmt" else [command, *patches]
    cargo += ["--manifest-path", str(collection / "wave-defense/testsuite/Cargo.toml"), *arguments]
    return subprocess.run(cargo, cwd=core, env=environment).returncode


if __name__ == "__main__":
    raise SystemExit(main())
