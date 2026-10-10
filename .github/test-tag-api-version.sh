#!/usr/bin/env bash
set -euo pipefail

script_root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
test_root=$(mktemp -d)
trap 'rm -rf -- "$test_root"' EXIT

git init --bare -q "$test_root/remote"
git init -b main -q "$test_root/collection"
cd "$test_root/collection"
git config user.name 'Package tag test'
git config user.email 'package-tag@example.invalid'
git remote add origin "$test_root/remote"
git -c commit.gpgsign=false commit -qm first --allow-empty
git push -q origin main
first=$(git rev-parse HEAD)

"$script_root/tag-api-version.sh" 1
test "$(git --git-dir="$test_root/remote" rev-parse refs/tags/api-v1)" = "$first"

git -c commit.gpgsign=false commit -qm second --allow-empty
git push -q origin main
second=$(git rev-parse HEAD)
"$script_root/tag-api-version.sh" 1
test "$(git --git-dir="$test_root/remote" rev-parse refs/tags/api-v1)" = "$second"

git -c commit.gpgsign=false commit -qm third --allow-empty
git push -q origin main
third=$(git rev-parse HEAD)
"$script_root/tag-api-version.sh" 2
test "$(git --git-dir="$test_root/remote" rev-parse refs/tags/api-v2)" = "$third"
test "$(git --git-dir="$test_root/remote" rev-parse refs/tags/api-v1)" = "$second"

# A delayed run must not replace a tag with an older collection commit.
git checkout -q --detach "$first"
"$script_root/tag-api-version.sh" 1
test "$(git --git-dir="$test_root/remote" rev-parse refs/tags/api-v1)" = "$second"

for invalid in '' 0 01 -1 '1/other'; do
    if "$script_root/tag-api-version.sh" "$invalid"; then
        echo "accepted invalid Package API version '$invalid'" >&2
        exit 1
    fi
done

echo 'Package API tag publication checks passed.'
