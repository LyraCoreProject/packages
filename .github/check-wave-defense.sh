#!/usr/bin/env bash
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
core_root=${1:?usage: check-wave-defense.sh /path/to/LyraCore COMMAND [CARGO-ARGS...]}
shift
"$collection_root/.github/check-core-tip.sh" "$core_root" \
    python3 "$collection_root/.github/run-wave-defense-tests.py" "$core_root" "$@"
