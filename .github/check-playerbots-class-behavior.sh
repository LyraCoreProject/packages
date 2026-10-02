#!/usr/bin/env bash
set -euo pipefail
collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
core_root=${1:?usage: check-playerbots-class-behavior.sh /path/to/LyraCore}
"$collection_root/.github/check-playerbots.sh" "$core_root" test --locked \
    --test playerbots_class_behavior --test playerbots_spell_ranks \
    -- --ignored --nocapture --test-threads=1
