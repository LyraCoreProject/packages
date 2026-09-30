#!/usr/bin/env bash
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
core_root=${1:?usage: check-playerbots.sh /path/to/LyraCore COMMAND [CARGO-ARGS...]}
shift
"$collection_root/.github/check-core-tip.sh" "$core_root" \
    python3 "$collection_root/.github/run-playerbots-tests.py" "$core_root" "$@"
