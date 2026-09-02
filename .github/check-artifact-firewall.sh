#!/usr/bin/env bash
# The no-committed-Package-Delta firewall: a Package's Script Artifact (kind "script") is
# package-authored Lua with no client-derived data, so it may be committed under
# data/.generated/ to ship with the Package. Every other artifact there is a Package Delta, and
# a Package Delta carries base-game data regenerated from the Operator's own client — the
# licensing firewall this collection cannot cross. A Package Delta has no "kind" member at all
# (LyraCore's `lyracore-package-delta` crate: version 1 shipped before there was a second kind),
# so its absence is itself the tell.
set -euo pipefail

collection_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$collection_root"

status=0
while IFS= read -r -d '' file; do
    kind=$(jq -r '.kind // "delta"' -- "$file")
    if [[ "$kind" != "script" ]]; then
        echo "$file: committed with kind \"$kind\" — the licensing firewall allows only a" \
            "Script Artifact (kind \"script\") under data/.generated/; a Package Delta is" \
            "regenerated author-side with 'packages build' and installed from source, never" \
            "committed" >&2
        status=1
    fi
done < <(git ls-files -z -- '*/data/.generated/*')

if [[ "$status" -eq 0 ]]; then
    echo "no committed Package Delta found under data/.generated/; only Script Artifacts are present."
fi

exit "$status"
