#!/usr/bin/env bash
set -euo pipefail

script_root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
test_root=$(mktemp -d)
trap 'rm -rf -- "$test_root"' EXIT

hash=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
valid_artifact="{\"kind\":\"script\",\"version\":1,\"package\":\"greeter\",\"source_hash\":\"$hash\",\"scripts\":[]}"
valid_identity="{\"artifact_hash\":\"sha256-v1:$hash\",\"bun_lock_hash\":\"sha256-v1:$hash\",\"bun_version\":\"1.3.7\",\"source_hash\":\"sha256-tree-v1:$hash\",\"toolchain_hash\":\"sha256-dir-v1:$hash\",\"version\":1}"

new_case() {
    local name=$1
    local root="$test_root/$name"
    mkdir -p "$root/.github"
    cp "$script_root/check-artifact-firewall.sh" "$root/.github/"
    git -C "$root" init -q
    echo "$root"
}

track() {
    local root=$1
    local path=$2
    local content=$3
    mkdir -p "$(dirname "$root/$path")"
    printf '%s\n' "$content" >"$root/$path"
    git -C "$root" add -- "$path"
}

expect_pass() {
    local root=$1
    if ! output=$(cd "$root" && ./.github/check-artifact-firewall.sh 2>&1); then
        echo "expected firewall pass in $root" >&2
        echo "$output" >&2
        exit 1
    fi
}

expect_refusal() {
    local root=$1
    local path=$2
    if output=$(cd "$root" && ./.github/check-artifact-firewall.sh 2>&1); then
        echo "expected firewall refusal for $path" >&2
        exit 1
    fi
    if [[ "$output" != *"$path"* ]]; then
        echo "firewall refusal did not name $path" >&2
        echo "$output" >&2
        exit 1
    fi
}

root=$(new_case valid-artifact)
track "$root" greeter/data/.generated/greeter.script.json "$valid_artifact"
expect_pass "$root"

root=$(new_case valid-pair)
track "$root" greeter/data/.generated/greeter.script.json "$valid_artifact"
track "$root" greeter/data/.generated/script.identity "$valid_identity"
expect_pass "$root"

root=$(new_case package-delta)
track "$root" greeter/data/.generated/spell.json \
    "{\"version\":1,\"package\":\"greeter\",\"source_hash\":\"$hash\",\"claims\":[]}"
expect_refusal "$root" greeter/data/.generated/spell.json

root=$(new_case arbitrary-json)
track "$root" greeter/data/.generated/notes.json '{"kind":"script","note":"not an artifact"}'
expect_refusal "$root" greeter/data/.generated/notes.json

root=$(new_case non-json)
track "$root" greeter/data/.generated/readme.txt 'not JSON'
expect_refusal "$root" greeter/data/.generated/readme.txt

root=$(new_case orphan-sidecar)
track "$root" greeter/data/.generated/script.identity "$valid_identity"
expect_refusal "$root" greeter/data/.generated/script.identity

root=$(new_case malformed-artifact)
track "$root" greeter/data/.generated/greeter.script.json '{"kind":"script"'
expect_refusal "$root" greeter/data/.generated/greeter.script.json

root=$(new_case malformed-sidecar)
track "$root" greeter/data/.generated/greeter.script.json "$valid_artifact"
track "$root" greeter/data/.generated/script.identity '{"version":1}'
expect_refusal "$root" greeter/data/.generated/script.identity

echo "artifact firewall cases passed."
