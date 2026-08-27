#!/usr/bin/env bash
# The datascripts half of core-tip CI (LyraCore#316): does every committed Package Delta artifact
# in this collection still match its recorded Build Identity, against LyraCore's CURRENT schema?
#
# Called from `check-core-tip.sh` with a linked `packages/` tree already in place, so this only has
# to prove the schema/typechecking half:
#
#   1. Two clean runs of `spacetime generate --lang typescript` must be byte-identical. This is the
#      one reproducibility assumption `lyracore packages check` and a Package author both build on
#      without ever verifying it themselves — the same steps this job performs it only performs
#      once, because they trust it.
#   2. The pinned Datascript dependencies install exactly as locked, and `tsc --noEmit` passes
#      against the regenerated typings.
#   3. `lyracore packages check` verifies every committed artifact's Build Identity against this
#      checkout: source, typings, authoring library and toolchain pins.
#
# What this job does NOT do: re-emit a Datascript. That needs a Base Snapshot, which is the
# Operator's own client-derived data and does not exist on a CI runner — see
# `docs/agents/cross-repo-cli.md` in LyraCore and `packages check`'s own handling of a missing
# snapshot. It never writes a regenerated artifact back to this checkout either; a diff here is a
# failure to fix by hand and re-commit, never something CI commits on the collection's behalf.

set -euo pipefail

core_root=${1:-}
if [[ -z "$core_root" || ! -f "$core_root/module/Cargo.toml" ]]; then
    echo "usage: $0 /path/to/LyraCore" >&2
    exit 2
fi
core_root=$(cd "$core_root" && pwd)
cd "$core_root"

typegen() {
    spacetime generate --lang typescript --module-path module --out-dir datascripts/generated \
        --build-options=--features=debug_reducers --no-config -y
}

echo "== regenerating datascripts/generated/ twice, to prove it is reproducible"
typegen
first=$(mktemp -d)
cp -r datascripts/generated/. "$first/"
rm -rf datascripts/generated
typegen
second=$(mktemp -d)
cp -r datascripts/generated/. "$second/"
if ! diff -rq "$first" "$second"; then
    echo "two clean runs of 'spacetime generate --lang typescript' produced different output — the" >&2
    echo "one reproducibility assumption this job exists to prove does not hold. Nothing else ran." >&2
    exit 1
fi
rm -rf "$first" "$second"
echo "reproducible."

echo "== installing the pinned Datascript dependencies and typechecking"
(cd datascripts && bun install --frozen-lockfile && bun ./node_modules/typescript/bin/tsc --noEmit)

echo "== lyracore packages check"
./lyracore packages check
