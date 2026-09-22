#!/usr/bin/env bash
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
core_root=${1:?usage: check-playerbots-class-behavior.sh /path/to/LyraCore}
core_root=$(cd "$core_root" && pwd)
test_path="$core_root/module/tests/playerbots_class_behavior.rs"
if [[ -e "$test_path" || -L "$test_path" ]]; then
    echo "refusing to replace $test_path" >&2
    exit 1
fi

# Compile beside Core's existing Standalone support; keep this Package's tests with its behavior.
cp "$collection_root/playerbots/tests/class_behavior.rs" "$test_path"
trap 'rm -- "$test_path"' EXIT
cd "$core_root"
"$collection_root/.github/check-core-tip.sh" "$core_root" \
    cargo test --locked -p lyracore-module --test playerbots_class_behavior \
    -- --ignored --nocapture --test-threads=1
