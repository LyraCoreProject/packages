#!/usr/bin/env bash
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
core_root=${1:?usage: check-playerbots-target-claims.sh /path/to/LyraCore}
core_root=$(cd "$core_root" && pwd)
test_path="$core_root/module/tests/playerbots_target_claims.rs"
if [[ -e "$test_path" || -L "$test_path" ]]; then
    echo "refusing to replace $test_path" >&2
    exit 1
fi

cp "$collection_root/playerbots/tests/target_claims.rs" "$test_path"
trap 'rm -- "$test_path"' EXIT
cd "$core_root"
"$collection_root/.github/check-core-tip.sh" "$core_root" \
    cargo test --locked -p lyracore-module --test playerbots_target_claims \
    -- --ignored --nocapture --test-threads=1
