#!/usr/bin/env python3
"""Run the reviewed lifecycle references and companion cases at explicit source pins."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time


COMPANION_CASES = (
    "playerbots_acceptance_human_and_four_companions_complete_the_fixed_route",
    "playerbots_acceptance_restart_transfer_and_lost_ack_apply_once",
)
CASE_SECONDS = 1800
EXECUTION_SECONDS = 5400
CLEANUP_SECONDS = 10
LOG_BYTES = 64 * 1024 * 1024
SUMMARY = re.compile(
    r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"(\d+) measured; (\d+) filtered out;"
)
TERMINAL = re.compile(r"^test (\S+) \.\.\.(?: (.*))?$")


def exited_unreaped(process: subprocess.Popen) -> bool:
    return os.waitid(
        os.P_PID,
        process.pid,
        os.WEXITED | os.WNOHANG | os.WNOWAIT,
    ) is not None


def group_has_live_member(pgid: int) -> bool:
    for stat_path in Path("/proc").glob("[0-9]*/stat"):
        try:
            stat = stat_path.read_text()
            fields = stat[stat.rfind(")") + 2 :].split()
            state, process_group = fields[0], int(fields[2])
        except (IndexError, OSError, ValueError):
            continue
        if process_group == pgid and state != "Z":
            return True
    return False


def exact_pass(log: str, expected: str) -> bool:
    summaries, results, pending = [], [], None
    lines = log.splitlines()
    for line in lines:
        if match := SUMMARY.match(line):
            summaries.append(match.groups())
        if match := TERMINAL.match(line):
            if pending is not None:
                return False
            name, tail = match.groups()
            if tail in {"ok", "FAILED", "ignored"}:
                results.append((name, tail))
            elif tail is not None and tail.startswith("ignored"):
                results.append((name, "ignored"))
            else:
                pending = name
        elif pending is not None and line in {"ok", "FAILED", "ignored"}:
            results.append((pending, line))
            pending = None
    return (
        pending is None
        and lines.count("running 1 test") == 1
        and len(summaries) == 1
        and summaries[0][:5] == ("ok", "1", "0", "0", "0")
        and results == [(expected, "ok")]
    )


def required_cases(manifest: dict) -> list[tuple[str, str, str]]:
    if manifest.get("schema") != "playerbots-action-lifecycle-v1":
        raise ValueError("unsupported lifecycle manifest")
    references = [cell["case"] for cell in manifest["cells"]] + manifest["guards"]
    cases = set()
    targets = {}
    for case in references:
        source = Path(case["source"])
        if source.is_absolute() or ".." in source.parts or len(source.parts) < 3:
            raise ValueError("invalid case source path")
        if source.parts[:2] == ("module", "tests"):
            crate = "lyracore-module"
        elif source.parts[:2] == ("gateway", "tests"):
            crate = "lyracore-gateway"
        else:
            raise ValueError("case source has no supported test crate")
        target, name = case["target"], case["name"]
        if not re.fullmatch(r"[a-z][a-z0-9_]*", target):
            raise ValueError("invalid Cargo test target")
        if not re.fullmatch(r"[a-z][a-z0-9_]*(?:::[a-z][a-z0-9_]*)*", name):
            raise ValueError("invalid Rust test name")
        if target in targets and targets[target] != crate:
            raise ValueError("ambiguous test target across crates")
        targets[target] = crate
        cases.add((crate, target, name))
    for name in COMPANION_CASES:
        cases.add(("lyracore-gateway", "playerbots_companion_acceptance", name))
    return sorted(cases)


def run_case(command: list[str], path: Path, cwd: Path, seconds: float) -> tuple[int, str]:
    with path.open("xb") as log:
        process = subprocess.Popen(command, cwd=cwd, stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        deadline = time.monotonic() + seconds
        failure = ""
        try:
            while not exited_unreaped(process):
                if time.monotonic() >= deadline:
                    failure = "case exceeded its execution time limit"
                    break
                if path.stat().st_size > LOG_BYTES:
                    failure = "case log exceeded 64 MiB"
                    break
                time.sleep(0.25)
        finally:
            # Cargo may exit before a test's Standalone or Gateway child.
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            cleanup_deadline = time.monotonic() + CLEANUP_SECONDS
            while group_has_live_member(process.pid) and time.monotonic() < cleanup_deadline:
                time.sleep(0.05)
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            status = process.wait()
        if not failure and time.monotonic() >= deadline:
            failure = "case exceeded its execution time limit"
        if not failure and path.stat().st_size > LOG_BYTES:
            failure = "case log exceeded 64 MiB"
        return status, failure


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--core-root", required=True, type=Path)
    parser.add_argument("--collection-root", required=True, type=Path)
    parser.add_argument("--core", required=True)
    parser.add_argument("--collection", required=True)
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    for root, expected in ((args.core_root, args.core), (args.collection_root, args.collection)):
        if not re.fullmatch(r"[0-9a-f]{40}", expected):
            parser.error("source pins must be complete commit identities")
        actual = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
        if actual != expected:
            parser.error("checkout does not match its source pin")
    source = args.core_root / "module/tests/playerbots_action_lifecycle_manifest.json"
    cases = required_cases(json.loads(source.read_text()))
    args.evidence.mkdir(parents=True, exist_ok=False)
    record = {"schema": "playerbots-test-runs-v1", "core": args.core,
              "collection": args.collection, "runs": []}
    deadline = time.monotonic() + EXECUTION_SECONDS
    for index, (crate, target, name) in enumerate(cases, 1):
        command = ["cargo", "test", "--manifest-path", str(args.core_root / "Cargo.toml"),
                   "--locked", "-p", crate, "--test", target, name, "--", "--ignored",
                   "--exact", "--nocapture", "--test-threads=1"]
        remaining = deadline - time.monotonic() - CLEANUP_SECONDS
        status, failure = None, "total execution budget exhausted before this case started"
        log_name, log_digest, passed = None, None, False
        if remaining > 0:
            log_path = args.evidence / f"{index:03}-{target}-{name.replace('::', '-')}.log"
            status, failure = run_case(command, log_path, args.core_root,
                                       min(CASE_SECONDS, remaining))
            raw = log_path.read_bytes()
            passed = status == 0 and not failure and exact_pass(raw.decode(errors="replace"), name)
            log_name, log_digest = log_path.name, hashlib.sha256(raw).hexdigest()
        record["runs"].append({"target": target, "name": name,
                               "status": "passed" if passed else "failed",
                               "command": command, "command_status": status, "failure": failure,
                               "log": log_name, "log_sha256": log_digest})
        (args.evidence / "executions.json").write_text(json.dumps(record, indent=2) + "\n")
        print(f"{index}/{len(cases)} {target}::{name}: {record['runs'][-1]['status']}", flush=True)
    for root, expected in ((args.core_root, args.core), (args.collection_root, args.collection)):
        actual = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
        if actual != expected:
            raise ValueError("source pin changed during execution")
    return int(any(run["status"] != "passed" for run in record["runs"]))


if __name__ == "__main__":
    raise SystemExit(main())
