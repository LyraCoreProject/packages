#!/usr/bin/env bash
set -euo pipefail

script_root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
test_root=$(mktemp -d)
trap 'rm -rf -- "$test_root"' EXIT

script_artifact='{"kind":"script"}'

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

expect_inventory_refusal() {
    local root=$1
    mkdir -p "$root/bin"
    printf '%s\n' '#!/usr/bin/env bash' 'exit 1' >"$root/bin/git"
    chmod +x "$root/bin/git"
    if output=$(cd "$root" && PATH="$root/bin:$PATH" ./.github/check-artifact-firewall.sh 2>&1); then
        echo "expected firewall refusal when Git cannot list tracked files" >&2
        exit 1
    fi
    if [[ "$output" != *"could not list Git-tracked generated Package files"* ]]; then
        echo "inventory refusal did not explain the failed inventory" >&2
        echo "$output" >&2
        exit 1
    fi
}

root=$(new_case valid-artifact)
track "$root" greeter/data/.generated/greeter.script.json "$script_artifact"
expect_pass "$root"

root=$(new_case valid-pair)
track "$root" greeter/data/.generated/greeter.script.json "$script_artifact"
track "$root" greeter/data/.generated/script.identity 'the later packages check owns this content'
expect_pass "$root"

root=$(new_case package-delta)
track "$root" greeter/data/.generated/spell.json \
    '{"version":1,"package":"greeter","claims":[]}'
expect_refusal "$root" greeter/data/.generated/spell.json

root=$(new_case non-script-artifact)
track "$root" greeter/data/.generated/notes.json '{"kind":"delta"}'
expect_refusal "$root" greeter/data/.generated/notes.json

root=$(new_case non-json)
track "$root" greeter/data/.generated/readme.txt 'not JSON'
expect_refusal "$root" greeter/data/.generated/readme.txt

root=$(new_case orphan-sidecar)
track "$root" greeter/data/.generated/script.identity 'not checked here'
expect_refusal "$root" greeter/data/.generated/script.identity

root=$(new_case malformed-artifact)
track "$root" greeter/data/.generated/greeter.script.json '{"kind":"script"'
expect_refusal "$root" greeter/data/.generated/greeter.script.json

root=$(new_case ambiguous-sidecar)
track "$root" greeter/data/.generated/first.json "$script_artifact"
track "$root" greeter/data/.generated/second.json "$script_artifact"
track "$root" greeter/data/.generated/script.identity 'one sidecar cannot identify two artifacts'
expect_refusal "$root" greeter/data/.generated/script.identity

root=$(new_case inventory-failure)
expect_inventory_refusal "$root"

echo "artifact firewall cases passed."
