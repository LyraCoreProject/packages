#!/usr/bin/env bash
set -euo pipefail

version=${1:-}
if [[ $# -ne 1 || ! "$version" =~ ^[1-9][0-9]*$ ]]; then
    echo "usage: $0 PACKAGE-API-VERSION" >&2
    exit 2
fi

checked_sha=$(git rev-parse HEAD)
main_ref=$(git ls-remote --exit-code origin refs/heads/main)
main_sha=${main_ref%%$'\t'*}
if [[ "$checked_sha" != "$main_sha" ]]; then
    echo "Skipping api-v$version: main has advanced beyond the checked collection."
    exit 0
fi

# Each version keeps its own tag when Core moves to a newer Package API.
git push origin "$checked_sha:refs/tags/api-v$version" --force
