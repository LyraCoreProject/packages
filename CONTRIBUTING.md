# Contributing

Each visible top-level directory is one Package. Keep unrelated Packages independent and do not add
a separate collection manifest.

Before opening a pull request:

1. Read LyraCore's `CODING_STANDARDS.md` and `CONTEXT.md` at the core revision you target.
2. Run `./.github/check-core-tip.sh /path/to/LyraCore` against a clean LyraCore checkout.
3. Confirm the Package contains no Blizzard client files, credentials, or private source.

A Package Delta is regenerated author-side with `lyracore packages build` and installed from
source; it never belongs in this collection. Only a Script Artifact (its JSON `kind` is
`"script"`) may be committed under a Package's `data/.generated/`, because it is package-authored
Lua with no client-derived data. CI enforces this: any other committed file under `data/.generated/`
fails the build by name. Drift between a committed Script Artifact and current core is caught
author-side, the same way, with `lyracore packages check`.

Pull requests must pass the `module` core-tip compatibility check before merge. Maintainers may use
their GitHub branch-protection bypass only to recover the repository or repair CI.
