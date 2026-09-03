#!/usr/bin/env bash
# The licensing firewall permits package-authored Script Artifacts and their Build Identity
# sidecars under data/.generated/. It refuses Package Deltas and every unrelated file there.
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$collection_root"

valid_script_artifact() {
    jq -e '
        def exact_keys($expected): (keys | sort) == ($expected | sort);
        def integer: type == "number" and floor == .;
        type == "object"
        and exact_keys(["kind", "package", "scripts", "source_hash", "version"])
        and .kind == "script"
        and .version == 1
        and (.package | type == "string" and test("^[a-z0-9._-]{1,64}$"))
        and (.source_hash | type == "string" and test("^[0-9a-f]{64}$"))
        and (.scripts | type == "array")
        and all(.scripts[];
            type == "object"
            and exact_keys(["enabled", "event", "name", "priority", "script_id", "source"])
            and (.script_id | integer and . >= 100000 and . <= 999999)
            and (.name | type == "string" and test("^[a-z0-9._-]{1,64}$"))
            and (.event | type == "string" and length > 0)
            and (.priority | integer and . >= -2147483648 and . <= 2147483647)
            and (.enabled | type == "boolean")
            and (.source | type == "string" and test("[^[:space:]]"))
        )
        and ([.scripts[].script_id] as $ids | ($ids | length) == ($ids | unique | length))
        and ([.scripts[].name] as $names | ($names | length) == ($names | unique | length))
    ' -- "$1" >/dev/null 2>&1
}

valid_script_identity() {
    jq -e '
        def exact_keys($expected): (keys | sort) == ($expected | sort);
        type == "object"
        and exact_keys([
            "artifact_hash",
            "bun_lock_hash",
            "bun_version",
            "source_hash",
            "toolchain_hash",
            "version"
        ])
        and .version == 1
        and (.artifact_hash | type == "string" and test("^sha256-v1:[0-9a-f]{64}$"))
        and (.bun_lock_hash | type == "string" and test("^sha256-v1:[0-9a-f]{64}$"))
        and (.bun_version | type == "string" and length > 0)
        and (.source_hash | type == "string" and test("^sha256-tree-v1:[0-9a-f]{64}$"))
        and (.toolchain_hash | type == "string" and test("^sha256-dir-v1:[0-9a-f]{64}$"))
    ' -- "$1" >/dev/null 2>&1
}

refuse() {
    echo "$1: $2. The licensing firewall permits only a valid Script Artifact and its" \
        "script.identity Build Identity sidecar under data/.generated/. A Package Delta is" \
        "regenerated author-side with 'packages build' and installed from source." >&2
    status=1
}

status=0
declare -A script_artifacts_by_dir=()
declare -a script_identities=()
mapfile -d '' generated_files < <(git ls-files -z -- '*/data/.generated/*')

for file in "${generated_files[@]}"; do
    directory=${file%/*}
    case ${file##*/} in
        script.identity)
            if valid_script_identity "$file"; then
                script_identities+=("$file")
            else
                refuse "$file" "not a valid Script Build Identity"
            fi
            ;;
        *.json)
            if valid_script_artifact "$file"; then
                script_artifacts_by_dir["$directory"]=$((${script_artifacts_by_dir["$directory"]:-0} + 1))
            else
                refuse "$file" "not a valid Script Artifact"
            fi
            ;;
        *)
            refuse "$file" "not a Script Artifact or Script Build Identity"
            ;;
    esac
done

for identity in "${script_identities[@]}"; do
    directory=${identity%/*}
    artifact_count=${script_artifacts_by_dir["$directory"]:-0}
    if [[ "$artifact_count" -eq 0 ]]; then
        refuse "$identity" "orphan Script Build Identity with no Script Artifact beside it"
    elif [[ "$artifact_count" -gt 1 ]]; then
        refuse "$identity" "ambiguous Script Build Identity beside more than one Script Artifact"
    fi
done

if [[ "$status" -eq 0 ]]; then
    echo "no committed Package Delta found under data/.generated/."
fi

exit "$status"
