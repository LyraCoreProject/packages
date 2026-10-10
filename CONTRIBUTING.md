# Contributing

Each visible top-level directory is one Package. Keep unrelated Packages independent and do not add
a separate collection manifest.

Before opening a pull request:

1. Read LyraCore's `CODING_STANDARDS.md` and `CONTEXT.md` at the core revision you target.
2. Run `./.github/check-core-tip.sh /path/to/LyraCore` against a clean LyraCore checkout.
3. Confirm the Package contains no Blizzard client files, credentials, or private source.

A Package Delta is regenerated author-side with `lyracore packages build` and installed from
source; it never belongs in this collection. A Script Artifact (its JSON `kind` is `"script"`) and
its `script.identity` Build Identity sidecar may be committed under a Package's
`data/.generated/`. The artifact contains package-authored Lua with no client-derived data. CI
names and refuses any other committed file there. `lyracore packages check` verifies the Build
Identity against the current core checkout.

Runtime Scripts declare a named function that takes its typed event and register it with
`events.player.onLogin`, another built-in event, or `events.package.on` for a Package Event.
Each source has one Event Binding. The Runtime Script Toolchain writes stable numeric IDs to the
Package-root `script-ids.json`; new sources need no `@id` directive. Commit that ledger with the
sources, Script Artifact and Build Identity. Keep entries for removed sources so later sources
cannot reuse their IDs.

Pull requests must pass the `module` core-tip compatibility check before merge. Maintainers may use
their GitHub branch-protection bypass only to recover the repository or repair CI.
