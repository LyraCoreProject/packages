#!/usr/bin/env bash
# The licensing firewall permits Script Artifacts and their exact Build Identity sidecar name under
# data/.generated/. It refuses Package Deltas and every unrelated file there. The Module parser and
# `packages check` own artifact and sidecar validity.
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$collection_root"

is_script_artifact() {
    jq -e '
        type == "object"
        and .kind == "script"
    ' -- "$1" >/dev/null 2>&1
}

refuse() {
    echo "$1: $2. The licensing firewall permits only a Script Artifact (top-level kind" \
        "\"script\") and its script.identity Build Identity sidecar under data/.generated/. A Package Delta is" \
        "regenerated author-side with 'packages build' and installed from source." >&2
    status=1
}

status=0
declare -A script_artifacts_by_dir=()
declare -a script_identities=()
inventory=$(mktemp)
trap 'rm -f -- "$inventory"' EXIT
if ! git ls-files -z -- '*/data/.generated/*' >"$inventory"; then
    echo "could not list Git-tracked generated Package files. The licensing firewall refuses to" >&2
    echo "continue without a complete inventory." >&2
    exit 1
fi
mapfile -d '' generated_files <"$inventory"

for file in "${generated_files[@]}"; do
    directory=${file%/*}
    case ${file##*/} in
        script.identity)
            script_identities+=("$file")
            ;;
        *)
            if is_script_artifact "$file"; then
                script_artifacts_by_dir["$directory"]=$((${script_artifacts_by_dir["$directory"]:-0} + 1))
            else
                refuse "$file" "not a Script Artifact"
            fi
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
